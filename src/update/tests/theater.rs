//! Theater Mode: entry / exit edges, the key policy, the mouse-route safety
//! net, the widened lyrics-blur gate, and the now-playing cover resolver.

use std::collections::HashMap;

use iced::{
    event::Status,
    keyboard::{Key, Modifiers, key::Named},
    widget::image::Handle,
};
use nokkvi_data::types::hotkey_config::HotkeyAction;

use crate::{
    Screen, View,
    app_message::{ContextMenuId, Message, NavigationMessage, OpenMenu, SplitViewMessage},
    test_helpers::*,
    update::theater::{TheaterKeyPolicy, theater_cover, theater_key_policy},
};

fn home_app() -> crate::Nokkvi {
    let mut app = test_app();
    app.screen = Screen::Home;
    app
}

fn press(app: &mut crate::Nokkvi, key: Key, modifiers: Modifiers) {
    let _ = app.handle_raw_key_event(key, modifiers, Status::Ignored);
}

fn press_f11(app: &mut crate::Nokkvi) {
    press(app, Key::Named(Named::F11), Modifiers::default());
}

fn press_escape(app: &mut crate::Nokkvi) {
    press(app, Key::Named(Named::Escape), Modifiers::default());
}

fn char_key(c: &str) -> Key {
    Key::Character(c.into())
}

/// A queue long enough that a slot-list step visibly moves the viewport.
fn seed_queue(app: &mut crate::Nokkvi, n: usize) {
    app.library.queue_songs = (0..n)
        .map(|i| make_queue_song(&format!("s{i}"), &format!("T{i}"), "A", "Al"))
        .collect();
}

fn byte_handle(tag: u8) -> Handle {
    Handle::from_bytes(vec![tag; 4])
}

// ----------------------------------------------------------------------------
// Entry and exit
// ----------------------------------------------------------------------------

#[test]
fn toggle_flips_active() {
    let mut app = home_app();
    press_f11(&mut app);
    assert!(app.theater.active, "F11 enters Theater Mode");
    press_f11(&mut app);
    assert!(!app.theater.active, "a second F11 leaves it");
}

#[test]
fn enter_refuses_on_login_screen() {
    let mut app = test_app();
    assert_eq!(app.screen, Screen::Login);
    press_f11(&mut app);
    assert!(!app.theater.active, "F11 on the Login screen does nothing");
    // The entry point itself refuses too (the IPC verb calls it directly).
    let _ = app.enter_theater();
    assert!(!app.theater.active);
}

#[test]
fn enter_clears_unmount_edge_state() {
    use crate::widgets::HoveredSlot;

    let mut app = home_app();
    app.current_view = View::Songs;
    app.songs_page.common.toolbar_hovered = true;
    app.songs_page.common.search_query = "abba".to_string();
    app.songs_page.common.search_input_focused = true;
    app.albums_page.common.search_input_focused = true;
    app.open_menu = Some(OpenMenu::Hamburger);
    app.cross_pane_drag.press_origin = Some(iced::Point::new(10.0, 10.0));
    app.cross_pane_drag.pressed_item = Some(3);
    app.albums_page.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
        slot_index: 0,
        item_index: 0,
        items_len: 1,
    });

    let _ = app.enter_theater();

    assert!(app.theater.active);
    assert!(
        !app.songs_page.common.toolbar_hovered,
        "reveal lock cleared"
    );
    assert!(
        !app.songs_page.common.search_input_focused,
        "search focus dropped even with a query (the box unmounts)"
    );
    assert!(!app.albums_page.common.search_input_focused);
    assert!(app.open_menu.is_none(), "open menu closed");
    assert!(
        app.cross_pane_drag.press_origin.is_none(),
        "drag press reset"
    );
    assert!(app.cross_pane_drag.pressed_item.is_none());
    assert!(
        app.albums_page.common.slot_list.hovered_slot.is_none(),
        "hovered slot cleared so a click-drag on the cover can't grab a hidden row"
    );
    assert_eq!(
        app.songs_page.common.search_query, "abba",
        "the search query survives"
    );
}

#[test]
fn exit_closes_open_menu() {
    let mut app = home_app();
    let _ = app.enter_theater();
    app.open_menu = Some(OpenMenu::Context {
        id: ContextMenuId::TheaterPanel,
        position: iced::Point::ORIGIN,
    });
    let _ = app.exit_theater();
    assert!(!app.theater.active);
    assert!(app.open_menu.is_none(), "the theater panel menu unmounts");
}

// ----------------------------------------------------------------------------
// Escape
// ----------------------------------------------------------------------------

#[test]
fn escape_leaves_theater_after_modals() {
    let mut app = home_app();
    let _ = app.enter_theater();
    app.eq_modal.open = true;

    press_escape(&mut app);
    assert!(!app.eq_modal.open, "the first Escape closes the EQ");
    assert!(app.theater.active, "and stays in theater");

    press_escape(&mut app);
    assert!(!app.theater.active, "the next Escape leaves theater");
}

#[test]
fn captured_escape_is_swallowed_in_theater() {
    // In theater no text input is mounted, so a Captured Escape is the one an
    // overlay menu just consumed closing itself. It must not also leave.
    let mut app = home_app();
    let _ = app.enter_theater();
    let _ = app.handle_raw_key_event(
        Key::Named(Named::Escape),
        Modifiers::default(),
        Status::Captured,
    );
    assert!(app.theater.active);
}

#[test]
fn escape_in_theater_does_not_touch_settings_or_panel() {
    let mut app = home_app();
    app.current_view = View::Settings;
    app.browsing_panel = Some(crate::views::BrowsingPanel::new());
    let _ = app.enter_theater();

    press_escape(&mut app);

    assert!(!app.theater.active);
    assert_eq!(app.current_view, View::Settings, "Settings not escaped");
    assert!(app.browsing_panel.is_some(), "the split view survives");
}

// ----------------------------------------------------------------------------
// Key policy
// ----------------------------------------------------------------------------

#[test]
fn policy_table_is_pinned() {
    use HotkeyAction as A;
    use TheaterKeyPolicy as P;

    let passthrough = [
        A::TogglePlay,
        A::ToggleRandom,
        A::ToggleRepeat,
        A::ToggleConsume,
        A::ToggleSoundEffects,
        A::CycleVisualization,
        A::ToggleEqModal,
        A::ToggleCrossfade,
        A::ToggleLyrics,
        A::ToggleBitPerfect,
        A::SeekBackward,
        A::SeekForward,
        A::ToggleTheater,
        A::Escape,
    ];
    let exit_then_perform = [
        A::SwitchToQueue,
        A::SwitchToAlbums,
        A::SwitchToArtists,
        A::SwitchToSongs,
        A::SwitchToGenres,
        A::SwitchToPlaylists,
        A::SwitchToRadios,
        A::SwitchToHarbour,
        A::SwitchToSettings,
        A::FocusSearch,
        A::ToggleBrowsingPanel,
        A::CenterOnPlaying,
        A::OpenTrawl,
        A::Roulette,
        A::NewSmartPlaylist,
        A::RefreshView,
        A::SaveQueueAsPlaylist,
        A::FindSimilar,
        A::FindTopSongs,
    ];
    let exit_only = [
        A::SlotListUp,
        A::SlotListDown,
        A::Activate,
        A::ExpandCenter,
        A::ShufflePlay,
        A::AddToQueue,
        A::RemoveFromQueue,
        A::ClearQueue,
        A::ToggleStar,
        A::IncreaseRating,
        A::DecreaseRating,
        A::GetInfo,
        A::MoveTrackUp,
        A::MoveTrackDown,
        A::TrawlSaveAsPlaylist,
        A::EditCenteredPlaylist,
        A::PrevSortMode,
        A::NextSortMode,
        A::ToggleSortOrder,
        A::EditUp,
        A::EditDown,
        A::ResetToDefault,
        A::SettingsCategoryNext,
        A::SettingsCategoryPrev,
    ];

    let mut expected: HashMap<HotkeyAction, TheaterKeyPolicy> = HashMap::new();
    for a in passthrough {
        expected.insert(a, P::Passthrough);
    }
    for a in exit_then_perform {
        expected.insert(a, P::ExitThenPerform);
    }
    for a in exit_only {
        expected.insert(a, P::ExitOnly);
    }

    let all: Vec<HotkeyAction> = HotkeyAction::ALL
        .iter()
        .chain(HotkeyAction::RESERVED.iter())
        .copied()
        .collect();
    assert_eq!(
        all.len(),
        expected.len(),
        "every action is classified exactly once in the table above"
    );
    for action in all {
        assert_eq!(
            theater_key_policy(action),
            expected[&action],
            "{action:?} changed class"
        );
    }
}

#[test]
fn exit_only_keys_leave_without_acting() {
    // Controls: outside theater, Tab in Settings synchronously drops the
    // settings search, and Shift+D on the Queue clears it with a toast, so the
    // in-theater assertions below are not green by construction. (The list
    // step itself runs in a follow-up Task, which tests cannot drive.)
    let mut control = home_app();
    control.current_view = View::Settings;
    control.settings_page.search_active = true;
    press(&mut control, Key::Named(Named::Tab), Modifiers::default());
    assert!(!control.settings_page.search_active);
    control.current_view = View::Queue;
    seed_queue(&mut control, 20);
    let toasts_before = control.toast.toasts.len();
    press(&mut control, char_key("D"), Modifiers::SHIFT);
    assert!(control.toast.toasts.len() > toasts_before);

    let mut app = home_app();
    app.current_view = View::Settings;
    app.settings_page.search_active = true;
    let _ = app.enter_theater();
    press(&mut app, Key::Named(Named::Tab), Modifiers::default());
    assert!(!app.theater.active, "Tab leaves");
    assert!(
        app.settings_page.search_active,
        "without driving the hidden view"
    );

    app.current_view = View::Queue;
    seed_queue(&mut app, 20);
    let _ = app.enter_theater();
    press(&mut app, Key::Named(Named::Enter), Modifiers::default());
    assert!(!app.theater.active, "Enter leaves");
    assert_eq!(app.queue_page.common.slot_list.viewport_offset, 0);

    let _ = app.enter_theater();
    let toasts_before = app.toast.toasts.len();
    press(&mut app, char_key("D"), Modifiers::SHIFT);
    assert!(!app.theater.active, "Shift+D leaves");
    assert_eq!(
        app.toast.toasts.len(),
        toasts_before,
        "and never clears the queue blind"
    );
    assert_eq!(app.current_view, View::Queue);
}

#[test]
fn exit_then_perform_keys_carry_through() {
    let mut app = home_app();
    app.current_view = View::Songs;
    let _ = app.enter_theater();
    press(&mut app, char_key("/"), Modifiers::default());
    assert!(!app.theater.active, "/ leaves");
    assert!(
        app.songs_page.common.search_input_focused,
        "and lands in the search box"
    );

    let _ = app.enter_theater();
    press(&mut app, char_key("2"), Modifiers::default());
    assert!(!app.theater.active, "2 leaves");
    assert_eq!(app.current_view, View::Albums, "and opens Albums");
}

#[test]
fn passthrough_keys_stay_in_theater() {
    let mut app = home_app();
    let _ = app.enter_theater();
    let random = app.modes.random;
    press(&mut app, char_key("x"), Modifiers::default());
    assert!(app.theater.active);
    assert_ne!(app.modes.random, random, "x still toggles shuffle");
}

#[test]
fn arrows_seek_in_theater_even_from_settings() {
    use crate::update::hotkeys::navigation::HorizontalArrowOwner;

    let mut app = home_app();
    app.current_view = View::Settings;
    assert_eq!(
        app.horizontal_arrow_owner(),
        HorizontalArrowOwner::SettingsEdit
    );
    let _ = app.enter_theater();
    assert_eq!(app.horizontal_arrow_owner(), HorizontalArrowOwner::View);
    press(
        &mut app,
        Key::Named(Named::ArrowRight),
        Modifiers::default(),
    );
    assert!(app.theater.active, "a seek key stays in theater");
}

// ----------------------------------------------------------------------------
// Mouse-route safety net
// ----------------------------------------------------------------------------

#[test]
fn switch_view_leaves_theater() {
    // The player bar's gear lands here (OpenSettings → SwitchView(Settings)).
    let mut app = home_app();
    app.current_view = View::Queue;
    let _ = app.enter_theater();
    let _ = app.update(Message::Navigation(NavigationMessage::SwitchView(
        View::Settings,
    )));
    assert!(!app.theater.active);
    assert_eq!(app.current_view, View::Settings);
}

#[test]
fn opening_the_browsing_panel_leaves_theater() {
    let mut app = home_app();
    app.current_view = View::Queue;
    let _ = app.enter_theater();
    let _ = app.update(Message::SplitView(SplitViewMessage::ToggleBrowsingPanel));
    assert!(app.browsing_panel.is_some());
    assert!(
        !app.theater.active,
        "a route that opens the split view never changes it invisibly"
    );
}

// ----------------------------------------------------------------------------
// Artwork gates
// ----------------------------------------------------------------------------

#[test]
fn blur_gate_admits_theater() {
    use nokkvi_data::types::player_settings::LyricsBackdropBlur;

    let mut app = home_app();
    app.current_view = View::Albums;
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Al")];
    app.scrobble.current_song_id = Some("s1".to_string());
    app.lyrics.enabled = true;
    app.settings.lyrics_backdrop_blur = LyricsBackdropBlur::Medium;
    app.artwork
        .large_artwork
        .put("album_s1".to_string(), byte_handle(1));

    assert!(
        app.lyrics_blur_task().is_none(),
        "outside theater the blur runs only on the Queue view"
    );
    let _ = app.enter_theater();
    assert!(app.lyrics_blur_task().is_some());
}

#[test]
fn theater_cover_prefers_blur_only_with_lyrics_then_large_then_mini() {
    let large = HashMap::from([("a".to_string(), byte_handle(1))]);
    let mini = HashMap::from([
        ("a".to_string(), byte_handle(2)),
        ("b".to_string(), byte_handle(3)),
    ]);
    let blurred = byte_handle(9);

    assert_eq!(
        theater_cover(&large, &mini, Some(&blurred), Some("a")).map(Handle::id),
        Some(blurred.id()),
        "the frost wins when the caller passes it (lyrics shown)"
    );
    assert_eq!(
        theater_cover(&large, &mini, None, Some("a")).map(Handle::id),
        large.get("a").map(Handle::id),
        "large before mini"
    );
    assert_eq!(
        theater_cover(&large, &mini, None, Some("b")).map(Handle::id),
        mini.get("b").map(Handle::id),
        "mini when the large art is cold"
    );
    assert!(theater_cover(&large, &mini, None, Some("zzz")).is_none());
    assert!(theater_cover(&large, &mini, None, None).is_none());
}

#[test]
fn stopped_queue_shows_sharp_cursor_cover_without_blur() {
    use nokkvi_data::types::player_settings::LyricsBackdropBlur;

    let mut app = home_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Al")];
    app.scrobble.current_song_id = Some("s1".to_string());
    app.lyrics.enabled = true;
    app.settings.lyrics_backdrop_blur = LyricsBackdropBlur::Medium;
    let sharp = byte_handle(1);
    let sharp_id = sharp.id();
    app.artwork.large_artwork.put("album_s1".to_string(), sharp);
    app.artwork.lyrics_blurred = Some(crate::state::LyricsBlurredCover {
        album_id: "album_s1".to_string(),
        source_id: sharp_id,
        level: LyricsBackdropBlur::Medium,
        handle: Some(byte_handle(9)),
    });
    // Stopped: the lyrics layer is transport-gated off, so the frost must not
    // show even though a matching blur is cached.
    assert!(app.queue_lyrics_panel_data().is_none());
    assert_eq!(
        app.theater_now_playing_cover().map(Handle::id),
        Some(sharp_id)
    );

    // Playing: the lyrics layer shows, so the frost takes over.
    app.playback.playing = true;
    assert!(app.queue_lyrics_panel_data().is_some());
    assert_ne!(
        app.theater_now_playing_cover().map(Handle::id),
        Some(sharp_id)
    );
}

#[test]
fn radio_uses_station_art() {
    let mut app = home_app();
    let station = nokkvi_data::types::radio_station::RadioStation {
        id: "radio_1".into(),
        name: "Test Radio".into(),
        stream_url: "http://example.invalid/r".into(),
        home_page_url: None,
        cover_art: None,
    };
    app.active_playback = crate::state::ActivePlayback::Radio(crate::state::RadioPlaybackState {
        station,
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });
    assert!(
        app.theater_now_playing_cover().is_none(),
        "tower placeholder"
    );
    let mini = byte_handle(3);
    let mini_id = mini.id();
    app.artwork.radio_art.put("radio_1".to_string(), mini);
    assert_eq!(
        app.theater_now_playing_cover().map(Handle::id),
        Some(mini_id)
    );
    let large = byte_handle(4);
    let large_id = large.id();
    app.artwork
        .radio_large_art
        .put("radio_1".to_string(), large);
    assert_eq!(
        app.theater_now_playing_cover().map(Handle::id),
        Some(large_id)
    );
}

#[test]
fn stopped_empty_queue_yields_none_without_panic() {
    let mut app = home_app();
    let _ = app.enter_theater();
    assert!(app.theater_now_playing_cover().is_none());
    let _ = app.view(iced::window::Id::unique());
}

#[test]
fn lyrics_surface_admits_theater() {
    // Resolves, next-track prefetch and the blur run only on a lyrics surface;
    // theater is one whatever view it was entered from.
    let mut app = home_app();
    app.current_view = View::Albums;
    assert!(!app.lyrics_surface_visible());
    let _ = app.enter_theater();
    assert!(app.lyrics_surface_visible());
}
