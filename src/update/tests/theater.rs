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

// ----------------------------------------------------------------------------
// The transient bar
// ----------------------------------------------------------------------------

mod transient {
    use std::time::{Duration, Instant};

    use nokkvi_data::types::player_settings::TheaterControls;

    use super::home_app;
    use crate::{
        app_message::{Message, TheaterMessage},
        state::{ChromeMotion, TheaterState},
        update::theater::{HIDE_DELAY, SLIDE_DURATION, chrome_target, cursor_hidden, slide_offset},
    };

    fn active_state(now: Instant) -> TheaterState {
        TheaterState {
            active: true,
            last_activity: Some(now),
            ..TheaterState::default()
        }
    }

    #[test]
    fn chrome_target_truth_table() {
        let now = Instant::now();
        let later = now + HIDE_DELAY + Duration::from_millis(1);
        let auto = TheaterControls::AutoHide;

        let s = active_state(now);
        assert!(chrome_target(&s, now, false, auto), "recent activity shows");
        assert!(
            !chrome_target(&s, later, false, auto),
            "idle past the delay hides"
        );
        assert!(
            chrome_target(&s, later, true, auto),
            "an open menu holds it"
        );

        let mut hovered = active_state(now);
        hovered.bar_hovered = true;
        assert!(
            chrome_target(&hovered, later, false, auto),
            "hover holds it"
        );

        let mut unfocused = active_state(now);
        unfocused.window_focused = false;
        unfocused.bar_hovered = true;
        assert!(
            !chrome_target(&unfocused, now, true, auto),
            "an unfocused window never shows the bar"
        );

        let mut never = active_state(now);
        never.last_activity = None;
        assert!(!chrome_target(&never, now, false, auto), "no activity yet");

        assert!(chrome_target(
            &s,
            later,
            false,
            TheaterControls::AlwaysShown
        ));
        assert!(!chrome_target(&s, now, true, TheaterControls::AlwaysHidden));
    }

    #[test]
    fn slide_offset_eases_between_endpoints() {
        let now = Instant::now();
        assert_eq!(
            slide_offset(
                ChromeMotion::Shown {
                    since: now,
                    from: 0.0
                },
                now
            ),
            0.0
        );
        let done = now - SLIDE_DURATION;
        assert_eq!(
            slide_offset(
                ChromeMotion::Hidden {
                    since: done,
                    from: 0.0
                },
                now
            ),
            1.0
        );
        // Monotone toward the target in between.
        let hide = ChromeMotion::Hidden {
            since: now,
            from: 0.0,
        };
        let mut prev = 0.0;
        for step in 1..=10 {
            let t = now + SLIDE_DURATION.mul_f32(step as f32 / 10.0);
            let off = slide_offset(hide, t);
            assert!(off >= prev, "monotone");
            assert!((0.0..=1.0).contains(&off));
            prev = off;
        }
        // A reversal mid-slide starts from the recorded offset.
        let back = ChromeMotion::Shown {
            since: now,
            from: 0.6,
        };
        assert!((slide_offset(back, now) - 0.6).abs() < 1e-6);
        assert!(slide_offset(back, now + SLIDE_DURATION / 2) < 0.6);
    }

    #[test]
    fn cursor_hidden_ignores_focus_and_setting_but_respects_menu() {
        let now = Instant::now();
        let later = now + HIDE_DELAY;
        let s = active_state(now);
        assert!(!cursor_hidden(&s, now, false));
        assert!(cursor_hidden(&s, later, false));
        assert!(!cursor_hidden(&s, later, true), "a menu keeps the cursor");
        let mut unfocused = active_state(now);
        unfocused.window_focused = false;
        assert!(cursor_hidden(&unfocused, later, false));
        let inactive = TheaterState::default();
        assert!(!cursor_hidden(&inactive, later, false));
    }

    #[test]
    fn unfocus_clears_bar_hover_and_hides() {
        let mut app = home_app();
        let _ = app.enter_theater();
        app.theater.bar_hovered = true;
        let _ = app.update(Message::WindowUnfocused);
        assert!(!app.theater.window_focused);
        assert!(!app.theater.bar_hovered);
        let now = Instant::now();
        assert!(!chrome_target(
            &app.theater,
            now,
            false,
            TheaterControls::AutoHide
        ));

        // Refocus alone does not bring the bar back; activity does.
        let _ = app.update(Message::WindowFocused);
        assert!(app.theater.window_focused);
        assert!(!chrome_target(
            &app.theater,
            Instant::now(),
            false,
            TheaterControls::AutoHide
        ));
        let _ = app.update(Message::Theater(TheaterMessage::Activity));
        assert!(chrome_target(
            &app.theater,
            Instant::now(),
            false,
            TheaterControls::AutoHide
        ));
    }

    #[test]
    fn activity_stamps_and_reveals() {
        let mut app = home_app();
        let _ = app.enter_theater();
        app.theater.last_activity = None;
        let _ = app.update(Message::Theater(TheaterMessage::Activity));
        assert!(app.theater.last_activity.is_some());
    }

    #[test]
    fn key_press_stamps_activity() {
        let mut app = home_app();
        let _ = app.enter_theater();
        app.theater.last_activity = None;
        super::press(
            &mut app,
            iced::keyboard::Key::Character("x".into()),
            iced::keyboard::Modifiers::default(),
        );
        assert!(app.theater.active);
        assert!(app.theater.last_activity.is_some());
    }

    #[test]
    fn bar_hover_tracks_enter_and_exit() {
        let mut app = home_app();
        let _ = app.enter_theater();
        let _ = app.update(Message::Theater(TheaterMessage::BarHover(true)));
        assert!(app.theater.bar_hovered);
        let _ = app.update(Message::Theater(TheaterMessage::BarHover(false)));
        assert!(!app.theater.bar_hovered);
    }

    #[test]
    fn exit_clears_bar_hover() {
        let mut app = home_app();
        let _ = app.enter_theater();
        app.theater.bar_hovered = true;
        let _ = app.exit_theater();
        assert!(!app.theater.bar_hovered);
    }

    #[test]
    fn tick_flips_chrome_once() {
        let mut app = home_app();
        let _ = app.enter_theater();
        assert!(app.theater.chrome.is_shown());
        let t1 = Instant::now() + HIDE_DELAY + Duration::from_millis(10);
        crate::update::theater::tick(&mut app, t1);
        let first = app.theater.chrome;
        assert!(!first.is_shown(), "idle past the delay flips to hidden");
        crate::update::theater::tick(&mut app, t1 + Duration::from_millis(16));
        assert_eq!(
            app.theater.chrome, first,
            "a second tick keeps the first flip"
        );
    }
}

// ----------------------------------------------------------------------------
// Reviewer R1 findings
// ----------------------------------------------------------------------------

mod r1 {
    use iced::keyboard::{Key, Modifiers, key::Named};

    use super::{char_key, home_app, press};
    use crate::{View, app_message::OpenMenu};

    #[test]
    fn strip_find_similar_leaves_theater_even_on_the_similar_tab() {
        let mut app = home_app();
        app.current_view = View::Queue;
        let mut panel = crate::views::BrowsingPanel::new();
        panel.active_view = crate::views::BrowsingView::Similar;
        app.browsing_panel = Some(panel);
        let _ = app.enter_theater();
        let _ = app.handle_find_similar("id".into(), "label".into());
        assert!(!app.theater.active, "the Similar results are shown");

        let _ = app.enter_theater();
        let _ = app.handle_find_top_songs("artist".into(), "label".into());
        assert!(!app.theater.active);
    }

    #[test]
    fn enter_clears_the_editor_slot_list_too() {
        use nokkvi_data::types::playlist_edit::PlaylistEditState;

        let mut app = home_app();
        let mut editor = crate::state::PlaylistEditorState::new(PlaylistEditState::new(
            "p1".into(),
            "P".into(),
            String::new(),
            false,
            Vec::new(),
        ));
        editor.common.slot_list.hovered_slot = Some(crate::widgets::HoveredSlot::Item {
            slot_index: 5,
            item_index: 5,
            items_len: 10,
        });
        editor.common.search_input_focused = true;
        editor.common.toolbar_hovered = true;
        app.playlist_editor = Some(editor);

        let _ = app.enter_theater();

        let editor = app.playlist_editor.as_ref().expect("editor survives");
        assert!(editor.common.slot_list.hovered_slot.is_none());
        assert!(!editor.common.search_input_focused);
        assert!(!editor.common.toolbar_hovered);
    }

    #[test]
    fn session_reset_leaves_theater() {
        let _guard = super::super::SSE_SLOT_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = home_app();
        let _ = app.enter_theater();
        let _ = app.reset_session_state();
        assert!(!app.theater.active);
    }

    #[test]
    fn enter_cancels_a_pending_find_and_expand() {
        let mut app = home_app();
        app.pending_expand.target = Some(crate::state::PendingExpand::Album {
            album_id: "a1".into(),
            for_browsing_pane: false,
        });
        let _ = app.enter_theater();
        assert!(app.pending_expand.target.is_none());
    }

    #[test]
    fn a_toggle_whose_hidden_target_is_already_on_only_leaves() {
        // Backtick over a hidden Settings view shows Settings, not the view
        // before it.
        let mut app = home_app();
        app.current_view = View::Settings;
        let _ = app.enter_theater();
        press(&mut app, char_key("`"), Modifiers::default());
        assert!(!app.theater.active);
        assert_eq!(app.current_view, View::Settings);

        // Ctrl+E with the split view open shows the split view.
        let mut app = home_app();
        app.current_view = View::Queue;
        app.browsing_panel = Some(crate::views::BrowsingPanel::new());
        let _ = app.enter_theater();
        press(&mut app, char_key("e"), Modifiers::CTRL);
        assert!(!app.theater.active);
        assert!(app.browsing_panel.is_some());

        // From elsewhere, backtick still leaves and opens Settings.
        let mut app = home_app();
        app.current_view = View::Albums;
        let _ = app.enter_theater();
        press(&mut app, char_key("`"), Modifiers::default());
        assert!(!app.theater.active);
        assert_eq!(app.current_view, View::Settings);
    }

    #[test]
    fn a_held_toggle_key_does_not_flicker() {
        let mut app = home_app();
        let raw = |repeat| {
            crate::Message::RawKeyEvent(
                Key::Named(Named::F11),
                Modifiers::default(),
                iced::event::Status::Ignored,
                repeat,
            )
        };
        let _ = app.update(raw(false));
        assert!(app.theater.active);
        for _ in 0..5 {
            let _ = app.update(raw(true));
        }
        assert!(app.theater.active, "repeats of the toggle are dropped");
        // A held seek key still repeats (bare Right seeks, stays in theater).
        let _ = app.update(crate::Message::RawKeyEvent(
            Key::Named(Named::Escape),
            Modifiers::default(),
            iced::event::Status::Ignored,
            true,
        ));
        assert!(!app.theater.active, "other repeated keys are delivered");
    }

    #[test]
    fn unfocus_closes_the_bar_menus() {
        let mut app = home_app();
        let _ = app.enter_theater();
        app.open_menu = Some(OpenMenu::PlayerModes);
        let _ = app.update(crate::Message::WindowUnfocused);
        assert!(app.open_menu.is_none(), "the bar hides, so its menu closes");

        app.open_menu = Some(OpenMenu::Hamburger);
        let _ = app.update(crate::Message::WindowUnfocused);
        assert!(app.open_menu.is_none());

        // The panel's own menu stays: the panel does not move.
        app.open_menu = Some(OpenMenu::Context {
            id: crate::app_message::ContextMenuId::TheaterPanel,
            position: iced::Point::ORIGIN,
        });
        let _ = app.update(crate::Message::WindowUnfocused);
        assert!(app.open_menu.is_some());
    }

    #[test]
    fn bare_key_press_helper_still_works() {
        let mut app = home_app();
        let _ = app.enter_theater();
        press(&mut app, Key::Named(Named::F11), Modifiers::default());
        assert!(!app.theater.active);
    }
}

#[test]
fn theater_lyrics_fit_the_panel_but_queue_lyrics_do_not() {
    let mut app = home_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Al")];
    app.scrobble.current_song_id = Some("s1".to_string());
    app.lyrics.enabled = true;
    app.playback.playing = true;
    assert!(
        !app.queue_lyrics_panel_data()
            .expect("lyrics layer shows")
            .fit_to_panel
    );
    assert!(
        app.theater_lyrics_panel_data()
            .expect("lyrics layer shows")
            .fit_to_panel
    );
}

mod cover_art {
    use nokkvi_data::types::player_settings::ArtworkCover;

    use super::{byte_handle, home_app};
    use crate::test_helpers::make_queue_song;

    /// Seed a playing queue track whose large art is cached.
    fn seeded() -> crate::Nokkvi {
        let mut app = home_app();
        app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Al")];
        app.scrobble.current_song_id = Some("s1".to_string());
        app.artwork
            .large_artwork
            .put("album_s1".to_string(), byte_handle(1));
        app
    }

    #[test]
    fn hidden_cover_blacks_out_and_fills_the_theater_panel() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let prior = crate::theme::artwork_cover();
        let app = seeded();

        crate::theme::set_artwork_cover(ArtworkCover::Show);
        assert!(!app.theater_cover_hidden());
        assert!(app.theater_now_playing_cover().is_some());

        crate::theme::set_artwork_cover(ArtworkCover::HideInTheater);
        assert!(app.theater_cover_hidden(), "theater hides the cover");
        assert!(app.theater_now_playing_cover().is_none());

        crate::theme::set_artwork_cover(ArtworkCover::HideEverywhere);
        assert!(app.theater_cover_hidden());

        crate::theme::set_artwork_cover(prior);
    }
}
