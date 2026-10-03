//! Root modal registry tests: the stacking order and `top_modal()`.
//!
//! Assertions target observable `Nokkvi` state only (`test_app()` has no
//! `AppService`).

use crate::{Nokkvi, test_helpers::test_app, update::modals::ActiveModal};

/// Open `modal` the way its own state reads open.
pub(super) fn open_modal(app: &mut Nokkvi, modal: ActiveModal) {
    match modal {
        ActiveModal::TextInputDialog => app.text_input_dialog.visible = true,
        ActiveModal::Eq => app.eq_modal.open = true,
        ActiveModal::About => app.about_modal.visible = true,
        ActiveModal::Info => app.info_modal.visible = true,
        ActiveModal::DefaultPlaylistPicker => {
            app.default_playlist_picker =
                Some(crate::widgets::default_playlist_picker::DefaultPlaylistPickerState::new(&[]));
        }
        ActiveModal::MilkdropPicker => {
            app.milkdrop.picker = Some(crate::widgets::milkdrop_picker::MilkdropPickerState::new(
                &app.milkdrop.library,
                None,
                false,
            ));
        }
        ActiveModal::Trawl => {
            app.trawl_modal = Some(crate::widgets::trawl_modal::TrawlModalState::default());
        }
    }
}

/// A Home-screen app on the Queue view with every root modal open.
pub(super) fn app_with_every_modal_open() -> Nokkvi {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    for modal in ActiveModal::STACK {
        open_modal(&mut app, modal);
    }
    app
}

#[test]
fn stack_order_is_pinned() {
    // The order is a UX decision (2026-10-03): the smaller, more transient
    // surface sits higher. A new modal fails to compile in `open_modal` above
    // until it is handled; give it a place here AND in `ActiveModal::STACK`.
    assert_eq!(
        ActiveModal::STACK,
        [
            ActiveModal::TextInputDialog,
            ActiveModal::Eq,
            ActiveModal::About,
            ActiveModal::Info,
            ActiveModal::DefaultPlaylistPicker,
            ActiveModal::MilkdropPicker,
            ActiveModal::Trawl,
        ]
    );
}

#[test]
fn no_modal_is_top_when_none_is_open() {
    let app = test_app();
    assert_eq!(app.top_modal(), None);
}

#[test]
fn each_modal_open_alone_is_the_top_modal() {
    for modal in ActiveModal::STACK {
        let mut app = test_app();
        open_modal(&mut app, modal);
        assert_eq!(app.top_modal(), Some(modal), "{modal:?} alone");
    }
}

#[test]
fn the_highest_open_modal_is_top() {
    // Open every modal, then drop them from the top down: each time the next
    // one in STACK takes over.
    let mut app = app_with_every_modal_open();
    for (i, &modal) in ActiveModal::STACK.iter().enumerate() {
        assert_eq!(app.top_modal(), Some(modal), "after {i} closed");
        match modal {
            ActiveModal::TextInputDialog => app.text_input_dialog.visible = false,
            ActiveModal::Eq => app.eq_modal.open = false,
            ActiveModal::About => app.about_modal.visible = false,
            ActiveModal::Info => app.info_modal.visible = false,
            ActiveModal::DefaultPlaylistPicker => app.default_playlist_picker = None,
            ActiveModal::MilkdropPicker => app.milkdrop.picker = None,
            ActiveModal::Trawl => app.trawl_modal = None,
        }
    }
    assert_eq!(app.top_modal(), None);
}
