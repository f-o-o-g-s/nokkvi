//! A TV remote's buttons through the key handler: the D-pad's Up/Down move the
//! lists, a button iced can't name binds by its Linux code, and the media keys
//! get past a modal. (OK → Enter and Back → Escape happen in the keyboard
//! subscription, before this handler; `hotkeys/remote.rs` tests them.)

use iced::{
    event::Status,
    keyboard::{Key, Modifiers, key::Named},
};
use nokkvi_data::types::hotkey_config::{HotkeyAction, KeyCode, KeyCombo};

use crate::{View, test_helpers::*, update::modals::ActiveModal};

/// KEY_VOICECOMMAND, the G20S voice button: iced has no name for it.
const VOICE: u32 = 582;

fn home_app(view: View) -> crate::Nokkvi {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = view;
    app
}

fn press(app: &mut crate::Nokkvi, key: Key, unnamed: Option<u32>) {
    let _ = app.update(crate::Message::RawKeyEvent(
        key,
        unnamed,
        Modifiers::default(),
        Status::Ignored,
        false,
    ));
}

#[test]
fn the_down_arrow_moves_a_library_list() {
    let mut app = home_app(View::Albums);
    // Moving down the list drops the search box's focus (the Tab behavior),
    // the one part of the move that lands synchronously.
    app.albums_page.common.search_input_focused = true;

    press(&mut app, Key::Named(Named::ArrowDown), None);

    assert!(
        !app.albums_page.common.search_input_focused,
        "a bare Down outside Settings should move the list, not do nothing"
    );
}

#[test]
fn the_arrows_step_a_picker_opened_over_settings() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage as M, PickerEntry};

    let mut app = home_app(View::Settings);
    let playlists = vec![
        make_playlist("p1", "Workout"),
        make_playlist("p2", "Chill"),
        make_playlist("p3", "Focus"),
    ];
    app.library.playlists.append_page(playlists, 3);
    let _ = app.handle_default_playlist_picker(M::Open);
    assert_eq!(app.top_modal(), Some(ActiveModal::DefaultPlaylistPicker));

    // Clear → Workout → Chill → Workout. Over Settings the arrows would edit a
    // hidden row; the picker on top takes them instead.
    press(&mut app, Key::Named(Named::ArrowDown), None);
    press(&mut app, Key::Named(Named::ArrowDown), None);
    press(&mut app, Key::Named(Named::ArrowUp), None);

    let centered = app
        .default_playlist_picker
        .as_ref()
        .and_then(|s| s.list.centered().cloned());
    assert!(
        matches!(&centered, Some(PickerEntry::Playlist { name, .. }) if name == "Workout"),
        "got {centered:?}"
    );
}

#[test]
fn an_unnamed_remote_button_binds_by_its_code() {
    let mut app = home_app(View::Queue);
    app.hotkey_config.set_binding(
        HotkeyAction::ToggleRandom,
        KeyCombo::key(KeyCode::Native(VOICE)),
    );

    // Another unnamed key does nothing.
    press(&mut app, Key::Unidentified, Some(VOICE + 1));
    assert!(!app.modes.random);

    press(&mut app, Key::Unidentified, Some(VOICE));
    assert!(app.modes.random, "the voice button should fire its binding");
}

#[test]
fn capturing_an_unnamed_remote_button_records_its_code() {
    use crate::views::settings::{SettingsAction, SettingsMessage};

    let mut page = crate::views::SettingsPage::new();
    page.capturing_hotkey = Some(HotkeyAction::TogglePlay);
    let action = page.update(
        SettingsMessage::HotkeyCaptured(Key::Unidentified, Some(VOICE), Modifiers::default()),
        &make_settings_view_data(),
    );

    match action {
        SettingsAction::WriteHotkeyBinding { action, combo } => {
            assert_eq!(action, HotkeyAction::TogglePlay);
            assert_eq!(combo, KeyCombo::key(KeyCode::Native(VOICE)));
        }
        other => panic!("expected the capture to write the binding, got {other:?}"),
    }
}

#[test]
fn the_media_keys_get_past_a_modal() {
    let mut app = home_app(View::Queue);
    app.eq_modal.open = true;
    assert_eq!(app.top_modal(), Some(ActiveModal::Eq));

    // Space toggles playback only without a modal; the media keys steer
    // playback under any modal, as MPRIS does.
    press(&mut app, Key::Named(Named::MediaPause), None);

    assert!(
        app.playback.paused,
        "the media Pause key should pause under EQ"
    );
    assert!(app.eq_modal.open, "and leave the modal open");
}

#[test]
fn every_modal_lets_the_media_keys_through() {
    for modal in ActiveModal::STACK {
        for &action in crate::update::modals::MEDIA_KEY_ACTIONS {
            assert!(modal.takes_action(action), "{modal:?} swallows {action:?}");
        }
    }
}

#[test]
fn the_home_page_key_goes_to_harbour() {
    let mut app = home_app(View::Albums);

    press(&mut app, Key::Named(Named::BrowserHome), None);

    assert_eq!(app.current_view, View::Harbour);
}

// ============================================================================
// Focus Navigation: the remote's Menu highlights the nav bar
// ============================================================================

mod nav_focus {
    use iced::keyboard::{Key, key::Named};
    use nokkvi_data::types::player_settings::NavLayout;

    use super::{home_app, press};
    use crate::{
        View,
        update::hotkeys::navigation::{NavFocusKey, nav_focus_key},
        widgets::nav_bar::{NavFocus, NavView},
    };

    /// Runs `check` with the nav layout set to `layout` under the theme lock,
    /// putting the old layout back afterwards: it is app-wide state other tests
    /// read.
    fn with_layout(layout: NavLayout, check: impl FnOnce()) {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let prior = crate::theme::nav_layout();
        crate::theme::set_nav_layout(layout);
        check();
        crate::theme::set_nav_layout(prior);
    }

    fn menu(app: &mut crate::Nokkvi) {
        press(app, Key::Named(Named::ContextMenu), None);
    }

    fn named(app: &mut crate::Nokkvi, key: Named) {
        press(app, Key::Named(key), None);
    }

    #[test]
    fn menu_highlights_the_current_views_tab() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Albums)));

            let mut app = home_app(View::Harbour);
            menu(&mut app);
            assert_eq!(app.nav_focus, Some(NavFocus::Harbour));

            // Settings has no tab: start on the first one.
            let mut app = home_app(View::Settings);
            menu(&mut app);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Queue)));
        });
    }

    #[test]
    fn the_arrows_walk_the_tabs_and_enter_opens_one() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            named(&mut app, Named::ArrowRight);
            named(&mut app, Named::ArrowRight);
            named(&mut app, Named::ArrowLeft);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Artists)));
            assert_eq!(app.current_view, View::Albums, "moving opens nothing");

            named(&mut app, Named::Enter);
            assert_eq!(app.current_view, View::Artists);
            assert_eq!(app.nav_focus, None, "opening a view drops the highlight");
        });
    }

    #[test]
    fn enter_on_the_longship_opens_harbour() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Radios);
            menu(&mut app);
            named(&mut app, Named::ArrowRight);
            assert_eq!(app.nav_focus, Some(NavFocus::Harbour));
            named(&mut app, Named::Enter);
            assert_eq!(app.current_view, View::Harbour);
        });
    }

    #[test]
    fn back_leaves_the_bar_and_does_nothing_else() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Albums);
            app.albums_page.common.search_query = "walkmen".to_string();
            menu(&mut app);
            named(&mut app, Named::Escape);
            assert_eq!(app.nav_focus, None);
            assert_eq!(
                app.albums_page.common.search_query, "walkmen",
                "Back only left the bar; it didn't also clear the search"
            );
        });
    }

    #[test]
    fn down_returns_to_the_list_and_up_stays() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            named(&mut app, Named::ArrowUp);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Albums)));
            named(&mut app, Named::ArrowDown);
            assert_eq!(app.nav_focus, None);
            assert_eq!(app.current_view, View::Albums);
        });
    }

    #[test]
    fn a_held_arrow_steps_the_highlight_once() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            for repeat in [false, true, true, true] {
                let _ = app.update(crate::Message::RawKeyEvent(
                    Key::Named(Named::ArrowRight),
                    None,
                    iced::keyboard::Modifiers::default(),
                    iced::event::Status::Ignored,
                    repeat,
                ));
            }
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Artists)));
        });
    }

    #[test]
    fn menu_again_drops_the_highlight() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            menu(&mut app);
            assert_eq!(app.nav_focus, None);
        });
    }

    #[test]
    fn another_key_drops_the_highlight_and_still_acts() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            press(&mut app, Key::Character("x".into()), None);
            assert_eq!(app.nav_focus, None);
            assert!(app.modes.random, "x still toggles random");
        });
    }

    #[test]
    fn the_sidebar_walks_with_up_and_down() {
        with_layout(NavLayout::Side, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            named(&mut app, Named::ArrowDown);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Artists)));
            named(&mut app, Named::ArrowLeft);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Artists)));
            named(&mut app, Named::ArrowRight);
            assert_eq!(app.nav_focus, None, "Right heads back to the list");
        });
    }

    #[test]
    fn the_none_layout_has_nothing_to_highlight() {
        with_layout(NavLayout::None, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            assert_eq!(app.nav_focus, None);
        });
    }

    #[test]
    fn a_view_switch_drops_the_highlight() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Albums);
            menu(&mut app);
            let _ = app.handle_switch_view(View::Songs);
            assert_eq!(app.nav_focus, None);
        });
    }

    #[test]
    fn menu_in_theater_leaves_it_and_highlights_the_bar() {
        with_layout(NavLayout::Top, || {
            let mut app = home_app(View::Queue);
            let _ = app.enter_theater();
            assert!(app.theater.active);
            menu(&mut app);
            assert!(!app.theater.active);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Queue)));
        });
    }

    #[test]
    fn the_highlight_stops_at_either_end() {
        assert_eq!(
            NavFocus::Tab(NavView::Queue).step(false),
            NavFocus::Tab(NavView::Queue)
        );
        assert_eq!(NavFocus::Harbour.step(true), NavFocus::Harbour);
        assert_eq!(NavFocus::Tab(NavView::Radios).step(true), NavFocus::Harbour);
        assert_eq!(
            NavFocus::Harbour.step(false),
            NavFocus::Tab(NavView::Radios)
        );
    }

    #[test]
    fn each_bar_runs_along_its_own_arrows() {
        use NavFocusKey::{Leave, Move, Open, Stay};
        let top = |k| nav_focus_key(k, false);
        assert_eq!(top(Named::ArrowLeft), Some(Move(false)));
        assert_eq!(top(Named::ArrowRight), Some(Move(true)));
        assert_eq!(top(Named::ArrowDown), Some(Leave));
        assert_eq!(top(Named::ArrowUp), Some(Stay));
        let side = |k| nav_focus_key(k, true);
        assert_eq!(side(Named::ArrowUp), Some(Move(false)));
        assert_eq!(side(Named::ArrowDown), Some(Move(true)));
        assert_eq!(side(Named::ArrowRight), Some(Leave));
        assert_eq!(side(Named::ArrowLeft), Some(Stay));
        for side_nav in [false, true] {
            assert_eq!(nav_focus_key(Named::Enter, side_nav), Some(Open));
            assert_eq!(nav_focus_key(Named::Escape, side_nav), Some(Leave));
            assert_eq!(nav_focus_key(Named::Space, side_nav), None);
        }
    }
}

// ============================================================================
// A remote's Back with nothing left to back out of goes up to the tabs
// (Plasma Bigscreen keeps the Menu key for its app switcher)
// ============================================================================

mod remote_back {
    use iced::{
        event::Status,
        keyboard::{Key, Modifiers, key::Named},
    };
    use nokkvi_data::types::player_settings::NavLayout;

    use super::home_app;
    use crate::{
        View,
        widgets::nav_bar::{NavFocus, NavView},
    };

    fn with_top_nav(check: impl FnOnce()) {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let prior = crate::theme::nav_layout();
        crate::theme::set_nav_layout(NavLayout::Top);
        check();
        crate::theme::set_nav_layout(prior);
    }

    fn key(app: &mut crate::Nokkvi, named: Named, status: Status) {
        let _ = app.update(crate::Message::RawKeyEvent(
            Key::Named(named),
            None,
            Modifiers::default(),
            status,
            false,
        ));
    }

    fn back(app: &mut crate::Nokkvi) {
        key(app, Named::BrowserBack, Status::Ignored);
    }

    #[test]
    fn back_at_rest_goes_up_to_the_tabs() {
        with_top_nav(|| {
            let mut app = home_app(View::Albums);
            back(&mut app);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Albums)));
            // And Back again comes back down.
            back(&mut app);
            assert_eq!(app.nav_focus, None);
        });
    }

    #[test]
    fn the_keyboards_escape_stays_put() {
        with_top_nav(|| {
            let mut app = home_app(View::Albums);
            key(&mut app, Named::Escape, Status::Ignored);
            assert_eq!(app.nav_focus, None);
        });
    }

    #[test]
    fn back_closes_a_popup_before_anything_else() {
        with_top_nav(|| {
            let mut app = home_app(View::Queue);
            app.eq_modal.open = true;
            back(&mut app);
            assert!(!app.eq_modal.open);
            assert_eq!(app.nav_focus, None);
        });
    }

    #[test]
    fn back_leaves_the_search_box_first() {
        with_top_nav(|| {
            let mut app = home_app(View::Albums);
            app.albums_page.common.search_input_focused = true;
            back(&mut app);
            assert!(!app.albums_page.common.search_input_focused);
            assert_eq!(app.nav_focus, None);
            back(&mut app);
            assert_eq!(app.nav_focus, Some(NavFocus::Tab(NavView::Albums)));
        });
    }

    #[test]
    fn back_that_closed_a_menu_goes_no_further() {
        with_top_nav(|| {
            let mut app = home_app(View::Albums);
            key(&mut app, Named::BrowserBack, Status::Captured);
            assert_eq!(app.nav_focus, None);
        });
    }

    #[test]
    fn back_in_the_none_layout_stays_put() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let prior = crate::theme::nav_layout();
        crate::theme::set_nav_layout(NavLayout::None);
        let mut app = home_app(View::Albums);
        back(&mut app);
        crate::theme::set_nav_layout(prior);
        assert_eq!(app.nav_focus, None);
        assert!(app.toast.toasts.is_empty(), "no toast on every Back");
    }
}
