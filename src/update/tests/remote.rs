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
