//! Tests for playback transport, modes, volume, and crossfade update handlers.

use crate::{View, app_message::PlaybackStateUpdate, test_helpers::*};

// ============================================================================
// Mode Flag Handlers (playback.rs)
// ============================================================================

#[test]
fn random_toggled_sets_flag() {
    let mut app = test_app();
    assert!(!app.modes.random);

    let _ = app.handle_random_toggled(true);
    assert!(app.modes.random);

    let _ = app.handle_random_toggled(false);
    assert!(!app.modes.random);
}

#[test]
fn repeat_toggled_sets_both_flags() {
    let mut app = test_app();
    assert!(!app.modes.repeat);
    assert!(!app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(true, false);
    assert!(app.modes.repeat);
    assert!(!app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(true, true);
    assert!(app.modes.repeat);
    assert!(app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(false, false);
    assert!(!app.modes.repeat);
    assert!(!app.modes.repeat_queue);
}

#[test]
fn consume_toggled_sets_flag() {
    let mut app = test_app();
    assert!(!app.modes.consume);

    let _ = app.handle_consume_toggled(true);
    assert!(app.modes.consume);

    let _ = app.handle_consume_toggled(false);
    assert!(!app.modes.consume);
}

// ============================================================================
// Playback State Machine (playback.rs)
// ============================================================================

fn make_playback_update() -> PlaybackStateUpdate {
    PlaybackStateUpdate {
        position: 42,
        duration: 200,
        playing: true,
        paused: false,
        title: "Test Song".to_string(),
        artist: "Test Artist".to_string(),
        album: "Test Album".to_string(),
        art_url: None,
        random: true,
        repeat: false,
        repeat_queue: false,
        consume: false,
        current_index: Some(0),
        song_id: Some("song_1".to_string()),
        format_suffix: "flac".to_string(),
        sample_rate: 44100,
        bitrate: 1411,
        live_icy_metadata: None,
        bpm: None,
    }
}

#[test]
fn playback_state_updated_maps_fields() {
    let mut app = test_app();
    let update = make_playback_update();

    let _ = app.handle_playback_state_updated(update);

    assert_eq!(app.playback.position, 42);
    assert_eq!(app.playback.duration, 200);
    assert!(app.playback.playing);
    assert!(!app.playback.paused);
    assert_eq!(app.playback.title, "Test Song");
    assert_eq!(app.playback.artist, "Test Artist");
    assert_eq!(app.playback.album, "Test Album");
    assert_eq!(app.playback.format_suffix, "flac");
    assert_eq!(app.playback.sample_rate, 44100);
    assert!(app.modes.random);
    assert!(!app.modes.repeat);
}

#[test]
fn playback_state_updated_detects_song_change() {
    let mut app = test_app();
    // Simulate first song playing
    app.scrobble.current_song_id = Some("old_song".to_string());
    app.scrobble.listening_time = 10.0;

    let update = make_playback_update(); // song_id = "song_1" (different)
    let _ = app.handle_playback_state_updated(update);

    // Scrobble state should be reset for new song
    assert_eq!(app.scrobble.current_song_id.as_deref(), Some("song_1"));
    assert_eq!(app.scrobble.listening_time, 0.0);
    assert!(!app.scrobble.submitted);
}

#[test]
fn playback_state_updated_same_song_no_reset() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.listening_time = 50.0;
    app.scrobble.last_position = 50.0;

    let mut update = make_playback_update();
    update.position = 55;
    update.song_id = Some("song_1".to_string()); // same song
    let _ = app.handle_playback_state_updated(update);

    // Listening time should accumulate, not reset
    assert!(app.scrobble.listening_time > 50.0);
}

#[test]
fn playback_state_tracks_listening_time_forward() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.last_position = 10.0;
    app.scrobble.listening_time = 0.0;

    let mut update = make_playback_update();
    update.position = 15; // 5 second forward delta
    update.song_id = Some("song_1".to_string());
    let _ = app.handle_playback_state_updated(update);

    assert!((app.scrobble.listening_time - 5.0).abs() < 0.1);
    assert_eq!(app.scrobble.last_position, 15.0);
}

#[test]
fn playback_state_ignores_seek_for_listening_time() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.last_position = 10.0;
    app.scrobble.listening_time = 5.0;

    // Big jump = seek, should not count
    let mut update = make_playback_update();
    update.position = 150; // 140 second jump
    update.song_id = Some("song_1".to_string());
    let _ = app.handle_playback_state_updated(update);

    // Listening time should NOT have increased by 140
    assert!(app.scrobble.listening_time < 10.0);
    // Position should still be updated for next delta
    assert_eq!(app.scrobble.last_position, 150.0);
}

// Volume Handlers (playback.rs) — toast-on-change unification
// ============================================================================

#[test]
fn volume_changed_sets_state_and_pushes_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_volume_changed(0.42);

    assert!((app.playback.volume - 0.42).abs() < f32::EPSILON);
    let last = app
        .toast
        .toasts
        .back()
        .expect("a volume toast should have been pushed");
    assert_eq!(last.message, "Volume: 42%");
    assert!(last.right_aligned, "volume toast is right-aligned");
}

#[test]
fn sfx_volume_changed_sets_state_and_pushes_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_sfx_volume_changed(0.7);

    assert!((app.sfx.volume - 0.7).abs() < f32::EPSILON);
    let last = app
        .toast
        .toasts
        .back()
        .expect("an sfx volume toast should have been pushed");
    assert_eq!(last.message, "SFX Volume: 70%");
    assert!(last.right_aligned, "sfx volume toast is right-aligned");
}

#[test]
fn volume_committed_sets_state_and_pushes_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_volume_committed(0.42);

    assert!((app.playback.volume - 0.42).abs() < f32::EPSILON);
    let last = app
        .toast
        .toasts
        .back()
        .expect("a volume toast should have been pushed");
    assert_eq!(last.message, "Volume: 42%");
    assert!(last.right_aligned, "volume toast is right-aligned");
}

#[test]
fn volume_committed_advances_throttle_inside_blocked_window() {
    // Pin the bug fix: VolumeCommitted must always advance the persist throttle
    // (and dispatch the persist task) even when VolumeChanged would be throttled.
    // Otherwise drag-release values within 500ms of the click-open value
    // never reach disk and are lost on next launch.
    let mut app = test_app();

    // First change opens the throttle window — persists.
    let _ = app.handle_volume_changed(0.30);
    let t1 = app
        .playback
        .volume_persist_throttle
        .expect("throttle should be set after first VolumeChanged");

    // Subsequent VolumeChanged within 500ms is blocked — throttle stays put.
    let _ = app.handle_volume_changed(0.50);
    let t1b = app
        .playback
        .volume_persist_throttle
        .expect("throttle still set");
    assert_eq!(
        t1, t1b,
        "VolumeChanged inside the 500ms window does NOT advance the throttle"
    );

    // VolumeCommitted MUST force-advance the throttle (force-persist semantics).
    let _ = app.handle_volume_committed(0.70);
    let t2 = app
        .playback
        .volume_persist_throttle
        .expect("throttle still set");
    assert!(
        t2 > t1,
        "VolumeCommitted advances throttle even inside the blocked window — \
         this is the slider-drag persistence fix"
    );

    // Final in-memory volume reflects the released value (not the blocked
    // intermediate change).
    assert!((app.playback.volume - 0.70).abs() < f32::EPSILON);
}

#[test]
fn volume_committed_sets_throttle_when_previously_unset() {
    // Even on the first event in a session (throttle = None), VolumeCommitted
    // sets the throttle so subsequent rapid VolumeChanged events get the
    // expected cooldown.
    let mut app = test_app();
    assert!(app.playback.volume_persist_throttle.is_none());

    let _ = app.handle_volume_committed(0.55);

    assert!(
        app.playback.volume_persist_throttle.is_some(),
        "VolumeCommitted seeds the throttle from the unset state"
    );
}

#[test]
fn sfx_volume_changed_clamps_above_one() {
    let mut app = test_app();
    let _ = app.handle_sfx_volume_changed(1.5);
    assert!((app.sfx.volume - 1.0).abs() < f32::EPSILON);
    assert_eq!(
        app.toast.toasts.back().map(|t| t.message.as_str()),
        Some("SFX Volume: 100%")
    );
}

// ============================================================================
// Crossfade Toggle (playback.rs)
// ============================================================================

#[test]
fn crossfade_toggle_flips_state() {
    let mut app = test_app();
    assert!(
        !app.engine.crossfade_enabled,
        "crossfade should default to false"
    );

    let _ = app.handle_toggle_crossfade();
    assert!(
        app.engine.crossfade_enabled,
        "first toggle should enable crossfade"
    );

    let _ = app.handle_toggle_crossfade();
    assert!(
        !app.engine.crossfade_enabled,
        "second toggle should disable crossfade"
    );
}

#[test]
fn crossfade_toggle_from_enabled() {
    let mut app = test_app();
    app.engine.crossfade_enabled = true;

    let _ = app.handle_toggle_crossfade();
    assert!(
        !app.engine.crossfade_enabled,
        "toggle from enabled should disable"
    );
}

// ============================================================================
// Settings Footer: Stale description_text After Sub-List Exit
// ============================================================================
//
// Bug: The description footer retains text from the item the user was on
// before entering a sub-list (color array or font picker). When the user
// escapes back to the main settings list, the footer shows the old description
// instead of the current center item's subtitle.

#[test]
fn settings_description_updates_after_color_sub_list_escape() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Visualizer category (Level 2)
    page.push_level(NavLevel::Category(SettingsTab::Visualizer));
    page.refresh_entries(&data);

    // 2. Navigate to a ColorArray item (peak gradient colors).
    //    Find the index of the peak_gradient_colors entry.
    let peak_gradient_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if item.key.contains("peak_gradient_colors"))
        })
        .expect("peak_gradient_colors entry should exist in Visualizer tab");

    // Position the slot list on the peak gradient colors item
    let total = page.cached_entries.len();
    page.slot_list.set_offset(peak_gradient_idx, total);
    page.update_description();

    // Capture the description text for the peak gradient item
    let peak_description = page.description_text.clone();
    assert!(
        !peak_description.is_empty(),
        "peak gradient colors item should have a description"
    );

    // 3. Activate to open the color sub-list
    let _ = page.update(SettingsMessage::EditActivate, &data);
    assert!(
        page.sub_list.is_some(),
        "EditActivate on ColorArray should open sub-list"
    );

    // Set a known-stale value while sub-list is active. In reality, the
    // description_text retains whatever it was before entering — this
    // exaggeration makes the test non-trivially detectible.
    page.description_text = "STALE FROM BEFORE COLOR SUB-LIST".to_string();

    // 4. Escape from the sub-list
    let _ = page.update(SettingsMessage::Escape, &data);
    assert!(page.sub_list.is_none(), "Escape should close the sub-list");

    // 5. description_text must be refreshed after sub-list exit
    assert_ne!(
        page.description_text, "STALE FROM BEFORE COLOR SUB-LIST",
        "description_text should be refreshed after color sub-list exit, \
         but it retained the stale value",
    );
}

#[test]
fn settings_description_updates_after_font_sub_list_escape() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Interface category where font_family lives
    page.push_level(NavLevel::Category(SettingsTab::Interface));
    page.refresh_entries(&data);

    // 2. Navigate to the font_family item
    let font_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if item.key.as_ref() == "font_family")
        })
        .expect("font_family entry should exist in Interface tab");

    let total = page.cached_entries.len();
    page.slot_list.set_offset(font_idx, total);
    page.update_description();

    let font_description = page.description_text.clone();
    assert!(
        !font_description.is_empty(),
        "font_family item should have a description"
    );

    // 3. Manually open font sub-list (simulating EditActivate)
    let all_fonts = vec!["Inter".to_string(), "Roboto".to_string()];
    page.font_sub_list = Some(crate::views::settings::FontSubListState {
        all_fonts: all_fonts.clone(),
        filtered_fonts: all_fonts,
        search_query: String::new(),
        slot_list: crate::widgets::SlotListView::new(),
        parent_offset: page.slot_list.viewport_offset,
    });

    // 4. Set a different description to prove staleness
    page.description_text = "STALE DESCRIPTION FROM BEFORE FONT PICKER".to_string();

    // 5. Escape from font sub-list
    let _ = page.update(SettingsMessage::Escape, &data);
    assert!(
        page.font_sub_list.is_none(),
        "Escape should close font sub-list"
    );

    // 6. description_text must be refreshed, not stale
    assert_ne!(
        page.description_text, "STALE DESCRIPTION FROM BEFORE FONT PICKER",
        "description_text should be refreshed after font sub-list exit"
    );
}

#[test]
fn settings_description_fresh_after_sub_list_then_pop_to_level1() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Visualizer
    page.push_level(NavLevel::Category(SettingsTab::Visualizer));
    page.refresh_entries(&data);

    // 2. Navigate to a ColorArray item and open sub-list
    let color_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if matches!(item.value, crate::views::settings::items::SettingValue::ColorArray(_)))
        })
        .expect("Should have at least one ColorArray entry in Visualizer tab");

    let total = page.cached_entries.len();
    page.slot_list.set_offset(color_idx, total);
    let _ = page.update(SettingsMessage::EditActivate, &data);
    assert!(page.sub_list.is_some(), "Should open sub-list");

    // 3. Escape sub-list
    let _ = page.update(SettingsMessage::Escape, &data);
    assert!(page.sub_list.is_none(), "Sub-list should be closed");

    // 4. Escape to pop back to Level 1 (CategoryPicker)
    let _ = page.update(SettingsMessage::Escape, &data);
    assert_eq!(
        *page.current_level(),
        NavLevel::CategoryPicker,
        "Should be back at CategoryPicker"
    );

    // 5. description_text should show a Level 1 header description,
    //    NOT the stale visualizer sub-item description.
    let level1_descriptions: Vec<&str> = SettingsTab::ALL.iter().map(|t| t.description()).collect();

    // The description should either be one of the tab descriptions or empty
    // (if somehow the cursor landed on nothing), but NOT a visualizer item subtitle.
    let is_valid_level1_desc = level1_descriptions.contains(&page.description_text.as_str())
        || page.description_text.is_empty();

    assert!(
        is_valid_level1_desc,
        "description_text should be a Level 1 tab description after popping to CategoryPicker, \
         got: '{}'",
        page.description_text,
    );
}

// ============================================================================
// Settings Footer: description_text Around Search Interactions
// ============================================================================

#[test]
fn settings_search_updates_description_from_stale() {
    use crate::views::settings::SettingsMessage;

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. At CategoryPicker, set up initial state
    page.refresh_entries(&data);
    page.update_description();

    // 2. Inject a known stale description
    page.description_text = "STALE BEFORE SEARCH".to_string();

    // 3. Search for something that yields items from deeper tabs
    let _ = page.update(SettingsMessage::SearchChanged("noise".to_string()), &data);
    assert!(
        !page.cached_entries.is_empty(),
        "'noise' should match at least 'Noise Reduction' from Visualizer tab"
    );

    // 4. Description should have been refreshed by SearchChanged → refresh_entries
    assert_ne!(
        page.description_text, "STALE BEFORE SEARCH",
        "SearchChanged should refresh description_text"
    );
}

#[test]
fn settings_search_clear_restores_level_description() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Visualizer
    page.push_level(NavLevel::Category(SettingsTab::Visualizer));
    page.refresh_entries(&data);
    page.update_description();

    // Capture description at offset 0 of Visualizer (should be the first non-header item)
    let viz_initial_desc = page.description_text.clone();

    // 2. Search for something
    let _ = page.update(SettingsMessage::SearchChanged("led".to_string()), &data);
    let _search_desc = page.description_text.clone();

    // 3. Clear search by sending empty query
    let _ = page.update(SettingsMessage::SearchChanged(String::new()), &data);

    // 4. Entries should be rebuilt for Visualizer (current level).
    //    Slot list was reset to offset 0, so description should match
    //    offset 0 of visualizer entries (same as step 1).
    assert_eq!(
        page.description_text, viz_initial_desc,
        "after clearing search, description should match the current level's entries at offset 0"
    );
}

#[test]
fn settings_escape_active_search_updates_description() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into General, search for something
    page.push_level(NavLevel::Category(SettingsTab::General));
    page.refresh_entries(&data);
    let _ = page.update(SettingsMessage::SearchChanged("scrobbl".to_string()), &data);

    // Inject a stale description to catch missing update_description
    page.description_text = "STALE BEFORE ESCAPE SEARCH".to_string();

    // 2. Escape clears active search
    let _ = page.update(SettingsMessage::Escape, &data);
    assert!(!page.search_active, "search should be deactivated");
    assert!(
        page.search_query.is_empty(),
        "search query should be cleared"
    );

    // 3. Description must be refreshed, not stale
    assert_ne!(
        page.description_text, "STALE BEFORE ESCAPE SEARCH",
        "description_text should be refreshed after Escape clears search"
    );
}

#[test]
fn settings_search_then_sub_list_then_escape_updates_description() {
    use crate::views::settings::SettingsMessage;

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. At CategoryPicker, search for "gradient" to find color arrays
    let _ = page.update(
        SettingsMessage::SearchChanged("gradient".to_string()),
        &data,
    );
    assert!(
        !page.cached_entries.is_empty(),
        "'gradient' should match entries"
    );

    // 2. Find a ColorArray entry in the search results
    let color_idx = page.cached_entries.iter().position(|e| {
        matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if matches!(item.value, crate::views::settings::items::SettingValue::ColorArray(_)))
    });

    if let Some(idx) = color_idx {
        // Navigate to it
        let total = page.cached_entries.len();
        page.slot_list.set_offset(idx, total);
        page.update_description();

        // 3. Open the sub-list
        let _ = page.update(SettingsMessage::EditActivate, &data);
        assert!(page.sub_list.is_some(), "should open color sub-list");

        // Inject stale description
        page.description_text = "STALE FROM SEARCH SUB-LIST".to_string();

        // 4. Escape from sub-list
        let _ = page.update(SettingsMessage::Escape, &data);
        assert!(page.sub_list.is_none(), "sub-list should close");

        // 5. Description should be refreshed (we're still in search mode)
        assert_ne!(
            page.description_text, "STALE FROM SEARCH SUB-LIST",
            "description should be refreshed after sub-list exit during search"
        );

        // Verify search_query is still intact (sub-list exit shouldn't clear search)
        assert_eq!(
            page.search_query, "gradient",
            "search query should survive sub-list exit"
        );
    }
}

#[test]
fn settings_search_from_sub_list_is_noop() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Visualizer and open a color sub-list
    page.push_level(NavLevel::Category(SettingsTab::Visualizer));
    page.refresh_entries(&data);

    let color_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if matches!(item.value, crate::views::settings::items::SettingValue::ColorArray(_)))
        })
        .expect("Visualizer should have a ColorArray entry");

    let total = page.cached_entries.len();
    page.slot_list.set_offset(color_idx, total);
    let _ = page.update(SettingsMessage::EditActivate, &data);
    assert!(page.sub_list.is_some(), "should be in sub-list");

    // 2. Capture current state
    let _desc_before = page.description_text.clone();
    let entries_before = page.cached_entries.len();

    // 3. Attempt to search while in sub-list — should be a no-op
    let _ = page.update(SettingsMessage::SearchChanged("test".to_string()), &data);

    // 4. Sub-list should still be open, entries unchanged
    assert!(page.sub_list.is_some(), "sub-list should remain open");
    assert_eq!(
        page.cached_entries.len(),
        entries_before,
        "entries should not change during sub-list search"
    );
    // search_query should NOT be set (sub-list handler ignores SearchChanged)
    // Actually, the sub-list handler returns None without modifying search_query
    // But wait — does search_query get modified? Let's check:
    assert!(
        page.search_query.is_empty(),
        "search_query should not be modified while in sub-list"
    );
}

#[test]
fn settings_search_header_does_not_use_tab_description() {
    use crate::views::settings::{SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // Search for "noise" — this returns a "General" section header (from
    // Visualizer's internal sections) followed by "Noise Reduction".
    // The "General" header shares its name with the top-level General tab.
    let _ = page.update(SettingsMessage::SearchChanged("noise".to_string()), &data);
    assert!(
        !page.cached_entries.is_empty(),
        "'noise' should yield results"
    );

    // At offset 0, the center item should be the "General" section header
    // from the Visualizer tab's search results.
    let total = page.cached_entries.len();
    let center_is_general_header = page
        .slot_list
        .get_center_item_index(total)
        .and_then(|idx| page.cached_entries.get(idx))
        .is_some_and(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Header { label, .. } if *label == "General")
        });

    if center_is_general_header {
        // The General *tab* description — this should NOT appear during search
        let general_tab_desc = SettingsTab::General.description();

        assert_ne!(
            page.description_text, general_tab_desc,
            "During search, a section header named 'General' should NOT be \
             mapped to the General tab's description '{general_tab_desc}'. \
             It should show just the section label.",
        );

        assert_eq!(
            page.description_text, "General",
            "During search, section headers should display their label, \
             not a tab description"
        );
    }
}

/// Exact repro from the user bug report:
/// 1. Open settings → search "peak" → Tab to navigate → Escape exits settings
/// 2. Re-open settings → description_text shows stale "Peak Gradient Mode" subtitle
///
/// Root cause: Tab sets `search_active = false` while keeping `search_query = "peak"`,
/// so Escape's `search_active && !search_query.is_empty()` check fails, sending the
/// page straight to ExitSettings without clearing description_text.
#[test]
fn settings_stale_description_after_tab_deactivated_search_then_exit() {
    use crate::views::settings::{SettingsMessage, SettingsTab};

    let mut app = test_app();
    app.current_view = View::Settings;

    // 1. Open settings and search for "peak"
    let _ = app.handle_settings(SettingsMessage::SearchChanged("peak".to_string()));
    assert!(
        !app.settings_page.cached_entries.is_empty(),
        "'peak' should match entries"
    );

    // 2. Tab navigates down — also sets search_active = false
    //    (This is what handle_slot_list_navigate_down does for Settings)
    app.settings_page.search_active = false;
    // search_query stays "peak" — this is the zombie state
    let _ = app.handle_settings(SettingsMessage::SlotListDown);

    // Capture the description, which should show peak gradient subtitle
    let desc_during_zombie = app.settings_page.description_text.clone();
    assert!(
        !desc_during_zombie.is_empty(),
        "description should be set during search results"
    );

    // 3. Escape — with search_active=false, this skips search-clearing
    //    and should exit settings. The description_text survives.
    let _ = app.handle_settings(SettingsMessage::Escape);

    // 4. Simulate re-opening settings
    //    In real app: handle_switch_view(Settings) returns Task::none()
    //    so no handle_settings call happens before the first render.
    //    The stale description_text is displayed.
    app.current_view = View::Settings;

    // If config_dirty is false and cached_entries is non-empty,
    // handle_settings won't auto-refresh on the first message.
    // So description_text must already be correct.
    //
    // The description should NOT be the stale zombie search result text.
    // It should be a valid Level 1 tab description or empty.
    let level1_descriptions: Vec<&str> = SettingsTab::ALL.iter().map(|t| t.description()).collect();

    let is_valid = app.settings_page.description_text.is_empty()
        || level1_descriptions.contains(&app.settings_page.description_text.as_str());

    assert!(
        is_valid,
        "After re-opening settings, description should be a Level 1 tab \
         description or empty, got stale: '{}'",
        app.settings_page.description_text,
    );
}

// ============================================================================
