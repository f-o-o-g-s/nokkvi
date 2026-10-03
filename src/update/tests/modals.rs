//! Root modal registry tests: the stacking order and `top_modal()`.
//!
//! Assertions target observable `Nokkvi` state only (`test_app()` has no
//! `AppService`).

use crate::{
    Nokkvi,
    test_helpers::{make_album, test_app},
    update::modals::ActiveModal,
    widgets::EqModalMessage,
};

/// Open `modal` the way its own state reads open.
pub(super) fn open_modal(app: &mut Nokkvi, modal: ActiveModal) {
    match modal {
        ActiveModal::TextInputDialog => app.text_input_dialog.visible = true,
        ActiveModal::Eq => app.eq_modal.open = true,
        ActiveModal::About => app.about_modal.visible = true,
        ActiveModal::Info => app.info_modal.open(
            nokkvi_data::types::info_modal::InfoModalItem::from_album_view_data(
                &make_album("a1", "One", "X"),
                None,
            ),
        ),
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

#[test]
fn each_open_modal_draws_its_own_overlay_and_no_other() {
    // The draw order comes from STACK by construction; this pins that each
    // arm of `modal_overlay` draws the modal it names.
    for modal in ActiveModal::STACK {
        let mut app = test_app();
        open_modal(&mut app, modal);
        for other in ActiveModal::STACK {
            assert_eq!(
                app.modal_overlay(other).is_some(),
                other == modal,
                "{other:?}'s overlay with {modal:?} open"
            );
        }
    }
}

// ----------------------------------------------------------------------------
// Escape
// ----------------------------------------------------------------------------

fn escape(app: &mut Nokkvi) {
    let _ = app.update(crate::Message::RawKeyEvent(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Ignored,
        false,
    ));
}

#[test]
fn escape_closes_the_modals_top_down() {
    let mut app = app_with_every_modal_open();
    for (i, &modal) in ActiveModal::STACK.iter().enumerate() {
        escape(&mut app);
        assert!(
            !app.modal_is_open(modal),
            "Escape {} closes {modal:?}",
            i + 1
        );
        for &below in &ActiveModal::STACK[i + 1..] {
            assert!(
                app.modal_is_open(below),
                "Escape {} leaves {below:?} open",
                i + 1
            );
        }
    }
}

#[test]
fn escape_on_eq_drops_the_preset_name_prompt() {
    // Escape used to set `open = false` directly, skipping Close, so the
    // half-typed preset-name prompt came back on the next open.
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    let _ = app.handle_eq_modal(EqModalMessage::Open);
    let _ = app.handle_eq_modal(EqModalMessage::SavePreset);
    assert!(app.eq_modal.save_mode);

    escape(&mut app);
    assert!(!app.eq_modal.open);
    let _ = app.handle_eq_modal(EqModalMessage::Open);

    assert!(
        !app.eq_modal.save_mode,
        "reopening EQ shows the sliders, not the stale prompt"
    );
}

#[test]
fn escape_closes_a_modal_before_the_roulette_spin_under_it() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = crate::View::Albums;
    app.library.albums.set_from_vec(vec![
        make_album("a1", "One", "X"),
        make_album("a2", "Two", "X"),
        make_album("a3", "Three", "X"),
        make_album("a4", "Four", "X"),
    ]);
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(
        crate::View::Albums,
    ));
    assert!(app.roulette.is_some(), "spin armed");
    open_modal(&mut app, ActiveModal::Trawl);

    escape(&mut app);

    assert!(
        app.trawl_modal.is_none(),
        "the modal on screen closes first"
    );
    assert!(
        app.roulette.is_some(),
        "the spin under it keeps going until the next Escape"
    );
}

// ----------------------------------------------------------------------------
// List keys (the key gate + the slot-list routing)
// ----------------------------------------------------------------------------

fn picker_offset(app: &Nokkvi) -> Option<usize> {
    app.default_playlist_picker
        .as_ref()
        .map(|p| p.slot_list.viewport_offset)
}

/// A playlist picker with one row under its Clear entry, so a step moves it.
fn open_picker_with_a_row(app: &mut Nokkvi) {
    let playlist = nokkvi_data::backend::playlists::PlaylistUIViewData {
        id: "p1".to_string(),
        name: "One".to_string(),
        comment: String::new(),
        duration: 0.0,
        song_count: 0,
        owner_name: String::new(),
        public: false,
        updated_at: String::new(),
        artwork_album_ids: Vec::new(),
        uploaded_image: None,
        is_smart: false,
        rules: None,
        evaluated_at: None,
        is_file_backed: false,
        sync: false,
        owner_id: String::new(),
        searchable_lower: String::new(),
        image: Default::default(),
    };
    app.default_playlist_picker =
        Some(crate::widgets::default_playlist_picker::DefaultPlaylistPickerState::new(&[playlist]));
}

#[test]
fn list_keys_stay_with_a_modal_above_the_picker() {
    // Tab steps the picker when it is on top; with EQ over it, the key
    // belongs to EQ (which takes none) and the hidden picker stays put.
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_picker_with_a_row(&mut app);
    open_modal(&mut app, ActiveModal::Eq);
    let before = picker_offset(&app);

    let _ = app.update(crate::Message::RawKeyEvent(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Ignored,
        false,
    ));

    assert_eq!(picker_offset(&app), before, "the picker under EQ stays put");
}

#[test]
fn enter_reaches_the_modal_over_a_roulette_spin() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = crate::View::Albums;
    app.library.albums.set_from_vec(vec![
        make_album("a1", "One", "X"),
        make_album("a2", "Two", "X"),
        make_album("a3", "Three", "X"),
        make_album("a4", "Four", "X"),
    ]);
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(
        crate::View::Albums,
    ));
    open_modal(&mut app, ActiveModal::DefaultPlaylistPicker);

    let _ = app.handle_slot_list_message(crate::app_message::SlotListMessage::ActivateCenter);

    assert!(
        app.default_playlist_picker.is_none(),
        "Enter chose the picker's centered entry"
    );
    assert!(
        app.roulette.as_ref().is_some_and(|r| r.decel.is_none()),
        "the spin under the picker keeps cruising"
    );
}

#[test]
fn list_timers_reach_the_view_under_a_modal() {
    // The scrollbar fade timer is bookkeeping for the view, not a key: a
    // modal opened before it fired must not strand the scrollbar visible.
    for modal in ActiveModal::STACK {
        let mut app = test_app();
        app.current_view = crate::View::Queue;
        app.screen = crate::Screen::Home;
        let slot_list = &mut app.queue_page.common.slot_list;
        slot_list.last_scrolled = Some(std::time::Instant::now());
        slot_list.scroll_generation_id = 5;
        open_modal(&mut app, modal);

        let _ = app.handle_slot_list_message(
            crate::app_message::SlotListMessage::ScrollbarFadeComplete(crate::View::Queue, 5),
        );

        assert!(
            app.queue_page.common.slot_list.last_scrolled.is_none(),
            "the fade lands under {modal:?}"
        );
    }
}

// ----------------------------------------------------------------------------
// Logout
// ----------------------------------------------------------------------------

#[test]
fn logout_discards_every_modal_without_running_its_close() {
    let _sse = super::SSE_SLOT_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut app = app_with_every_modal_open();
    // The Trawl save dialog's Cancel reopens Trawl; logout must not.
    app.text_input_dialog.open(
        "Save Mix as Playlist",
        "",
        "Playlist name...",
        crate::widgets::text_input_dialog::TextInputDialogAction::CreatePlaylistFromTrawl(vec![
            "s1".to_string(),
        ]),
    );
    let _ = app.handle_eq_modal(EqModalMessage::SavePreset);

    let _ = app.reset_session_state();

    for modal in ActiveModal::STACK {
        assert!(!app.modal_is_open(modal), "{modal:?} closed at logout");
    }
    assert!(
        !app.eq_modal.save_mode,
        "no half-typed preset name greets the next login"
    );
}
