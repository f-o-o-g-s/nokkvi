//! Tests for open-menu update handlers.

use crate::{View, test_helpers::*};

// ============================================================================
// Open-Menu Handler (menus.rs)
// ============================================================================

#[test]
fn set_open_menu_opens_when_none() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    assert_eq!(app.open_menu, None);

    let _ = app.handle_set_open_menu(Some(OpenMenu::Hamburger));
    assert_eq!(app.open_menu, Some(OpenMenu::Hamburger));
}

#[test]
fn set_open_menu_replaces_existing_menu() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    app.open_menu = Some(OpenMenu::Hamburger);

    let _ = app.handle_set_open_menu(Some(OpenMenu::PlayerModes));
    assert_eq!(app.open_menu, Some(OpenMenu::PlayerModes));
}

#[test]
fn set_open_menu_none_closes_any_open_menu() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    app.open_menu = Some(OpenMenu::PlayerModes);

    let _ = app.handle_set_open_menu(None);
    assert_eq!(app.open_menu, None);
}

#[test]
fn set_open_menu_none_when_already_none_is_idempotent() {
    let mut app = test_app();
    assert_eq!(app.open_menu, None);

    let _ = app.handle_set_open_menu(None);
    assert_eq!(app.open_menu, None);
}

#[test]
fn switch_view_closes_open_menu() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    app.open_menu = Some(OpenMenu::Hamburger);

    let _ = app.handle_switch_view(View::Albums);
    assert_eq!(
        app.open_menu, None,
        "navigating to a new view should close any open overlay menu"
    );
}

#[test]
fn window_resized_closes_open_menu() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    app.open_menu = Some(OpenMenu::PlayerModes);

    let _ = app.handle_window_resized(1280.0, 720.0);
    assert_eq!(
        app.open_menu, None,
        "resizing the window invalidates anchored overlays — close them"
    );
}
