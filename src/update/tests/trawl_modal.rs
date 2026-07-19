//! Trawl modal handler tests — open/close lifecycle, search machinery
//! (generation stale-drop), crate mutations, CTA flows, and the keyboard
//! gates (suppression / nav pass-through / Ctrl+Enter = Play Mix).
//!
//! Assertions target observable `Nokkvi` state only; `app_service` is `None`
//! in `test_app()`, so shell tasks yield no async work — the synchronous
//! state transitions ARE the contract under test.

use nokkvi_data::types::{
    batch::BatchItem,
    library_search::LibrarySearchResults,
    trawl::{TrawlBlend, TrawlMinLength, TrawlSeed},
};

use crate::{
    test_helpers::test_app,
    widgets::trawl_modal::{TrawlModalMessage, TrawlModalState, TrawlTrayControl},
};

fn seed(id: &str) -> TrawlSeed {
    TrawlSeed::new(BatchItem::Album(id.to_string()), id, "Artist")
}

fn open_modal(app: &mut crate::Nokkvi) {
    let _ = app.handle_trawl_modal(TrawlModalMessage::Open);
    assert!(app.trawl_modal.is_some(), "modal must open");
}

fn results_with_genre() -> Box<LibrarySearchResults> {
    use nokkvi_data::types::genre::Genre;
    Box::new(LibrarySearchResults {
        genres: vec![Genre {
            id: "phonk".into(),
            name: "Phonk".into(),
            album_count: 12,
            song_count: 140,
        }],
        ..Default::default()
    })
}

fn seed_radio_playback(app: &mut crate::Nokkvi) {
    use crate::state::{ActivePlayback, RadioPlaybackState};
    app.active_playback = ActivePlayback::Radio(RadioPlaybackState {
        station: nokkvi_data::types::radio_station::RadioStation {
            id: "r1".into(),
            name: "Test".into(),
            stream_url: "http://example.invalid/stream".into(),
            home_page_url: None,
            cover_art: None,
        },
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });
}

fn send_raw_key(
    app: &mut crate::Nokkvi,
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> iced::Task<crate::Message> {
    app.update(crate::Message::RawKeyEvent(
        key,
        modifiers,
        iced::event::Status::Ignored,
    ))
}

// ---- lifecycle -------------------------------------------------------------

#[test]
fn open_initializes_state_and_close_clears_it() {
    let mut app = test_app();
    assert!(app.trawl_modal.is_none());

    open_modal(&mut app);
    let state = app.trawl_modal.as_ref().expect("open");
    assert!(state.search_query.is_empty());
    assert!(state.search_results.is_none());
    assert!(!state.search_loading);

    let _ = app.handle_trawl_modal(TrawlModalMessage::Close);
    assert!(app.trawl_modal.is_none(), "close clears the modal");
}

#[test]
fn crate_survives_close_and_reopen() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));
    app.trawl_crate.blend = TrawlBlend::Weighted;
    app.trawl_crate.min_length = TrawlMinLength::S120;

    let _ = app.handle_trawl_modal(TrawlModalMessage::Close);
    open_modal(&mut app);

    assert_eq!(app.trawl_crate.len(), 1, "seeds survive close");
    assert_eq!(app.trawl_crate.blend, TrawlBlend::Weighted);
    assert_eq!(app.trawl_crate.min_length, TrawlMinLength::S120);
}

// ---- search machinery --------------------------------------------------------

#[test]
fn search_bumps_generation_every_keystroke_even_clears() {
    let mut app = test_app();
    open_modal(&mut app);
    let g0 = app.trawl_search_generation;

    let _ = app.handle_trawl_modal(TrawlModalMessage::SearchChanged("bu".into()));
    assert_eq!(app.trawl_search_generation, g0.wrapping_add(1));
    assert!(app.trawl_modal.as_ref().is_some_and(|s| s.search_loading));

    // Clearing below the threshold ALSO bumps — a late in-flight result must
    // not repopulate an emptied query.
    let _ = app.handle_trawl_modal(TrawlModalMessage::SearchChanged("b".into()));
    assert_eq!(app.trawl_search_generation, g0.wrapping_add(2));
    let state = app.trawl_modal.as_ref().expect("open");
    assert!(state.search_results.is_none());
    assert!(!state.search_loading, "sub-threshold clears loading");
}

#[test]
fn search_loaded_stale_generation_is_dropped() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_search_generation = 7;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_loading = true;
    }

    let _ = app.handle_trawl_modal(TrawlModalMessage::SearchLoaded {
        generation: 6, // stale
        result: Ok(results_with_genre()),
    });

    let state = app.trawl_modal.as_ref().expect("open");
    assert!(state.search_results.is_none(), "stale result dropped");
    assert!(state.search_loading, "newer search's loading untouched");
}

#[test]
fn search_loaded_current_generation_stores_results() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_search_generation = 4;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_query = "ph".into();
        state.search_loading = true;
    }

    let _ = app.handle_trawl_modal(TrawlModalMessage::SearchLoaded {
        generation: 4,
        result: Ok(results_with_genre()),
    });

    let state = app.trawl_modal.as_ref().expect("open");
    assert!(!state.search_loading);
    assert!(
        state
            .search_results
            .as_ref()
            .is_some_and(|r| r.genres.len() == 1)
    );
}

#[test]
fn search_loaded_error_clears_results_and_toasts() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_search_generation = 2;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_results = Some(*results_with_genre());
        state.search_loading = true;
    }

    let _ = app.handle_trawl_modal(TrawlModalMessage::SearchLoaded {
        generation: 2,
        result: Err("boom".into()),
    });

    let state = app.trawl_modal.as_ref().expect("modal stays open");
    assert!(state.search_results.is_none(), "stale rows must not linger");
    assert!(!state.search_loading);
    assert!(!app.toast.toasts.is_empty(), "search failure toasts");
}

// ---- crate mutations ----------------------------------------------------------

#[test]
fn click_result_row_toggles_seed_in_and_out() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_search_generation = 1;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_query = "ph".into();
        state.search_results = Some(*results_with_genre());
    }

    // Row 0 = "Genres" header, row 1 = the Phonk result.
    let _ = app.handle_trawl_modal(TrawlModalMessage::ClickRow(1));
    assert!(
        app.trawl_crate
            .contains(&BatchItem::Genre("Phonk".to_string())),
        "click adds the seed"
    );
    let _ = app.handle_trawl_modal(TrawlModalMessage::ClickRow(1));
    assert!(app.trawl_crate.is_empty(), "second click removes it");
}

#[test]
fn activating_a_header_row_is_a_noop() {
    let mut app = test_app();
    open_modal(&mut app);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_query = "ph".into();
        state.search_results = Some(*results_with_genre());
    }

    let _ = app.handle_trawl_modal(TrawlModalMessage::ClickRow(0)); // header
    assert!(app.trawl_crate.is_empty(), "headers add nothing");

    let _ = app.handle_trawl_modal(TrawlModalMessage::ClickRow(99)); // out of range
    assert!(app.trawl_crate.is_empty());
}

#[test]
fn remove_seed_and_clear_crate() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));
    app.trawl_crate.add(seed("al2"));

    let _ = app.handle_trawl_modal(TrawlModalMessage::RemoveSeed(0));
    assert_eq!(app.trawl_crate.len(), 1);
    assert!(app.trawl_crate.contains(&BatchItem::Album("al2".into())));

    app.trawl_crate.add(seed("al3"));
    let _ = app.handle_trawl_modal(TrawlModalMessage::ClearCrate);
    assert!(app.trawl_crate.is_empty());
}

#[test]
fn weight_steppers_clamp_to_bounds() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));

    let _ = app.handle_trawl_modal(TrawlModalMessage::DecWeight(0));
    assert_eq!(app.trawl_crate.seeds[0].weight, 1, "floor is 1");

    for _ in 0..9 {
        let _ = app.handle_trawl_modal(TrawlModalMessage::IncWeight(0));
    }
    assert_eq!(app.trawl_crate.seeds[0].weight, 5, "cap is 5");

    let _ = app.handle_trawl_modal(TrawlModalMessage::DecWeight(0));
    assert_eq!(app.trawl_crate.seeds[0].weight, 4);
}

#[test]
fn set_blend_and_min_length_write_to_the_crate() {
    let mut app = test_app();
    open_modal(&mut app);

    let _ = app.handle_trawl_modal(TrawlModalMessage::SetBlend(TrawlBlend::ShuffleAll));
    assert_eq!(app.trawl_crate.blend, TrawlBlend::ShuffleAll);

    let _ = app.handle_trawl_modal(TrawlModalMessage::SetMinLength(TrawlMinLength::Off));
    assert_eq!(app.trawl_crate.min_length, TrawlMinLength::Off);

    let _ = app.handle_trawl_modal(TrawlModalMessage::SetMaxLength(
        nokkvi_data::types::trawl::TrawlMaxLength::S480,
    ));
    assert_eq!(
        app.trawl_crate.max_length,
        nokkvi_data::types::trawl::TrawlMaxLength::S480
    );

    let _ = app.handle_trawl_modal(TrawlModalMessage::SetRating(
        nokkvi_data::types::trawl::TrawlRatingFilter::R4,
    ));
    assert_eq!(
        app.trawl_crate.rating,
        nokkvi_data::types::trawl::TrawlRatingFilter::R4
    );

    let _ = app.handle_trawl_modal(TrawlModalMessage::SetMaxTracks(
        nokkvi_data::types::trawl::TrawlMaxTracks::T50,
    ));
    assert_eq!(
        app.trawl_crate.max_tracks,
        nokkvi_data::types::trawl::TrawlMaxTracks::T50
    );
}

// ---- CTA flows -------------------------------------------------------------------

#[test]
fn play_mix_transitions_radio_to_queue() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));
    seed_radio_playback(&mut app);

    let _ = app.handle_trawl_modal(TrawlModalMessage::PlayMix);
    assert!(
        matches!(app.active_playback, crate::state::ActivePlayback::Queue),
        "guard_play_action must transition radio → queue"
    );
    assert!(
        app.trawl_modal.is_some(),
        "modal stays open until the resolve completes"
    );
}

#[test]
fn play_mix_with_empty_crate_is_a_noop() {
    let mut app = test_app();
    open_modal(&mut app);
    seed_radio_playback(&mut app);

    let _ = app.handle_trawl_modal(TrawlModalMessage::PlayMix);
    assert!(
        matches!(app.active_playback, crate::state::ActivePlayback::Radio(_)),
        "empty crate: no guard, no play"
    );
}

#[test]
fn play_mix_completed_ok_closes_the_modal() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));

    let _ = app.handle_trawl_modal(TrawlModalMessage::PlayMixCompleted(Ok(())));
    assert!(app.trawl_modal.is_none(), "success closes the modal");
    assert!(
        !app.trawl_crate.is_empty(),
        "the crate survives playing — tweak and replay is the workflow"
    );
}

#[test]
fn play_mix_completed_err_keeps_modal_open_and_toasts() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));

    let _ = app.handle_trawl_modal(TrawlModalMessage::PlayMixCompleted(Err(
        "Mix is empty — every song was under 1:00. Lower the minimum length.".into(),
    )));
    assert!(
        app.trawl_modal.is_some(),
        "failure keeps the modal open so the user can adjust"
    );
    assert!(!app.toast.toasts.is_empty(), "failure toasts");
}

#[test]
fn add_mix_completed_ok_toasts_the_count_and_stays_open() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));

    let _ = app.handle_trawl_modal(TrawlModalMessage::AddMixCompleted(Ok(42)));
    assert!(app.trawl_modal.is_some(), "enqueue keeps the modal open");
    assert!(
        app.toast
            .toasts
            .iter()
            .any(|t| t.message.contains("42 songs")),
        "toast names the resolved count"
    );
}

#[test]
fn add_mix_completed_singular_count_is_pluralized_properly() {
    let mut app = test_app();
    open_modal(&mut app);

    let _ = app.handle_trawl_modal(TrawlModalMessage::AddMixCompleted(Ok(1)));
    assert!(
        app.toast
            .toasts
            .iter()
            .any(|t| t.message.contains("1 song") && !t.message.contains("1 songs")),
        "singular count reads '1 song'"
    );
}

// ---- keyboard gates -------------------------------------------------------------

#[test]
fn bare_key_suppressed_while_trawl_modal_open() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    app.trawl_modal = Some(TrawlModalState::default());
    assert!(!app.modes.random);

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("x".into()),
        iced::keyboard::Modifiers::empty(),
    );

    assert!(
        !app.modes.random,
        "ToggleRandom must be suppressed while the trawl modal is open"
    );
}

#[test]
fn nav_key_passes_through_to_trawl_modal() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    app.trawl_search_generation = 1;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_query = "ph".into();
        state.search_results = Some(*results_with_genre());
    }
    let before = app
        .trawl_modal
        .as_ref()
        .map(|s| s.slot_list.viewport_offset);

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
        iced::keyboard::Modifiers::empty(),
    );

    let after = app
        .trawl_modal
        .as_ref()
        .map(|s| s.slot_list.viewport_offset);
    assert_ne!(
        before, after,
        "Tab (SlotListDown) must reach the trawl modal's nav route"
    );
}

#[test]
fn ctrl_enter_in_modal_plays_the_mix() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));
    seed_radio_playback(&mut app);

    // Ctrl+Enter resolves to ShufflePlay → ActivateCenterShuffled; inside the
    // trawl modal the one playable thing is the mix itself.
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::CTRL,
    );

    assert!(
        matches!(app.active_playback, crate::state::ActivePlayback::Queue),
        "Ctrl+Enter must route to PlayMix (guard ran)"
    );
}

#[test]
fn enter_in_modal_toggles_centered_seed_not_play() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    app.trawl_search_generation = 1;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_query = "ph".into();
        state.search_results = Some(*results_with_genre());
        // Center the result row (index 1; header is 0).
        state.slot_list.set_selected(1, 2);
    }

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::empty(),
    );

    assert!(
        app.trawl_crate
            .contains(&BatchItem::Genre("Phonk".to_string())),
        "Enter toggles the centered result into the crate"
    );
}

#[test]
fn escape_closes_the_trawl_modal_and_the_crate_survives() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));

    // Escape resolves through the ClearSearch cascade; the modal's tier
    // returns a Close task — drive the message it produces like prod would.
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        iced::keyboard::Modifiers::empty(),
    );
    // The cascade emits Task::done(Close); tests don't run tasks, so route
    // the same Close message through the dispatcher to complete the hop.
    let _ = app.update(crate::Message::TrawlModal(TrawlModalMessage::Close));

    assert!(app.trawl_modal.is_none(), "Escape closes the editor");
    assert_eq!(app.trawl_crate.len(), 1, "the crate persists");
}

#[test]
fn escape_tier_prefers_the_picker_on_a_double_open() {
    // Both-open is practically unreachable, but the tiers must agree: the
    // picker wins everywhere (gate, slot-list intercept, Escape cascade).
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    app.default_playlist_picker =
        Some(crate::widgets::default_playlist_picker::DefaultPlaylistPickerState::new(&[]));

    let task = app.handle_clear_search();
    drop(task);
    // The cascade returned the PICKER's close message — the trawl modal is
    // untouched by this Escape.
    assert!(
        app.trawl_modal.is_some(),
        "picker tier fires first; trawl modal survives this Escape"
    );
}

#[test]
fn queue_header_anchor_button_opens_the_modal() {
    let mut app = test_app();
    assert!(app.trawl_modal.is_none());

    let _ = app.handle_queue(crate::views::QueueMessage::OpenTrawl);

    assert!(
        app.trawl_modal.is_some(),
        "the queue header's anchor button opens the trawl modal"
    );
}

// ---- global hotkey (bare `t`) -------------------------------------------------

#[test]
fn t_hotkey_opens_the_trawl_modal_from_a_library_view() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("t".into()),
        iced::keyboard::Modifiers::empty(),
    );

    assert!(app.trawl_modal.is_some(), "bare t opens the trawl modal");
}

#[test]
fn t_hotkey_is_inert_in_settings() {
    let mut app = test_app();
    app.current_view = crate::View::Settings;
    app.screen = crate::Screen::Home;

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("t".into()),
        iced::keyboard::Modifiers::empty(),
    );

    assert!(
        app.trawl_modal.is_none(),
        "the mix builder does not open over Settings"
    );
}

#[test]
fn t_hotkey_is_swallowed_while_another_modal_is_open() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    app.eq_modal.open = true;

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("t".into()),
        iced::keyboard::Modifiers::empty(),
    );

    assert!(
        app.trawl_modal.is_none(),
        "the modal-open gate swallows the trawl hotkey"
    );
}

// ---- search focus lifecycle (Tab unfocuses, `/` refocuses) ---------------------

#[test]
fn open_and_typing_mark_the_search_focused() {
    let mut app = test_app();
    open_modal(&mut app);
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "Open focuses the search field"
    );

    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
    }
    let _ = app.handle_trawl_modal(TrawlModalMessage::SearchChanged("bu".into()));
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "typing proves focus"
    );
}

#[test]
fn tab_unfocuses_the_modal_search_but_backspace_does_not() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    app.trawl_search_generation = 1;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_query = "ph".into();
        state.search_results = Some(*results_with_genre());
    }

    // Tab (SlotListDown) doubles as "exit search" — the regular views' rule.
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
        iced::keyboard::Modifiers::empty(),
    );
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| !s.search_input_focused),
        "Tab exits the search field"
    );

    // Backspace (SlotListUp) must keep focus — it deletes text.
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = true;
    }
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
        iced::keyboard::Modifiers::empty(),
    );
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "Backspace keeps the search focused for deletion"
    );
}

#[test]
fn slash_refocuses_the_modal_search() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
    }

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("/".into()),
        iced::keyboard::Modifiers::empty(),
    );

    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "/ refocuses the modal's search from the list"
    );
}

// ---- tray keyboard cursor (Shift+Tab ring, Left/Right value cycling) ------------

/// Like [`send_raw_key`], but with `Status::Captured` — a focused text_input
/// swallowed the event. Shift+Tab/Shift+Backspace must still act (the
/// `is_shift_nav` carve-out), which is exactly what these tests pin.
fn send_raw_key_captured(
    app: &mut crate::Nokkvi,
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> iced::Task<crate::Message> {
    app.update(crate::Message::RawKeyEvent(
        key,
        modifiers,
        iced::event::Status::Captured,
    ))
}

fn shift_tab(app: &mut crate::Nokkvi) {
    let _ = send_raw_key(
        app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
        iced::keyboard::Modifiers::SHIFT,
    );
}

fn shift_backspace(app: &mut crate::Nokkvi) {
    let _ = send_raw_key(
        app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
        iced::keyboard::Modifiers::SHIFT,
    );
}

/// Bare Left/Right — the PrevSortMode/NextSortMode bindings.
fn arrow(app: &mut crate::Nokkvi, right: bool) {
    let named = if right {
        iced::keyboard::key::Named::ArrowRight
    } else {
        iced::keyboard::key::Named::ArrowLeft
    };
    let _ = send_raw_key(
        app,
        iced::keyboard::Key::Named(named),
        iced::keyboard::Modifiers::empty(),
    );
}

fn tray_cursor(app: &crate::Nokkvi) -> Option<TrawlTrayControl> {
    app.trawl_modal.as_ref().and_then(|s| s.tray_cursor)
}

fn open_modal_over(app: &mut crate::Nokkvi, view: crate::View) {
    app.current_view = view;
    app.screen = crate::Screen::Home;
    open_modal(app);
}

#[test]
fn shift_tab_enters_the_tray_and_unfocuses_search() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "Open focuses the search field"
    );

    // The search text_input holds iced focus, so the event arrives Captured —
    // the is_shift_nav carve-out must still let Shift+Tab through.
    let _ = send_raw_key_captured(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
        iced::keyboard::Modifiers::SHIFT,
    );

    assert_eq!(
        tray_cursor(&app),
        Some(TrawlTrayControl::Blend),
        "Shift+Tab enters the tray at the first control"
    );
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| !s.search_input_focused),
        "entering the tray unfocuses the search field so arrows go live"
    );
}

#[test]
fn shift_tab_cycles_the_ring_and_wraps_through_none() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);

    let expected = [
        Some(TrawlTrayControl::Blend),
        Some(TrawlTrayControl::MinLength),
        Some(TrawlTrayControl::MaxLength),
        Some(TrawlTrayControl::Rating),
        Some(TrawlTrayControl::MaxTracks),
        None,
    ];
    for want in expected {
        shift_tab(&mut app);
        assert_eq!(
            tray_cursor(&app),
            want,
            "the ring walks every control then wraps to None"
        );
    }
}

#[test]
fn shift_backspace_reverse_cycles_the_ring() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
    }

    // From None, backward enters at the LAST control — pins that the gate
    // admits SettingsCategoryMotion(false), not just the forward direction.
    shift_backspace(&mut app);
    assert_eq!(tray_cursor(&app), Some(TrawlTrayControl::MaxTracks));
    shift_backspace(&mut app);
    assert_eq!(tray_cursor(&app), Some(TrawlTrayControl::Rating));
}

#[test]
fn shift_backspace_while_search_focused_leaves_the_tray_alone() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused)
    );

    // Shift held from a capital + Backspace mid-typing: the Captured event
    // already deleted a character — it must not ALSO move the ring or yank
    // iced focus out of the field.
    let _ = send_raw_key_captured(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
        iced::keyboard::Modifiers::SHIFT,
    );

    assert_eq!(tray_cursor(&app), None, "the ring must not move mid-typing");
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "the search field keeps focus for further deletion"
    );
}

#[test]
fn captured_keys_never_drive_the_tray_even_when_rebound() {
    use nokkvi_data::types::hotkey_config::{HotkeyAction, KeyCode, KeyCombo};

    // A user may invert the category pair (Shift+Backspace = NEXT). While
    // typing in the search field that keypress is a character deletion — it
    // must not also enter the ring or yank focus, regardless of which
    // DIRECTION (or which tray action) the combo resolves to: the swallow is
    // status-keyed and action-agnostic.
    let mut app = test_app();
    app.hotkey_config.set_binding(
        HotkeyAction::SettingsCategoryNext,
        KeyCombo::shift(KeyCode::Backspace),
    );
    open_modal_over(&mut app, crate::View::Queue);

    let _ = send_raw_key_captured(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
        iced::keyboard::Modifiers::SHIFT,
    );

    assert_eq!(
        tray_cursor(&app),
        None,
        "a captured press never moves the ring, whatever it resolves to"
    );
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "the search field keeps focus mid-typing"
    );
}

#[test]
fn shift_backspace_with_stale_focus_flag_still_enters_the_ring() {
    // The mid-typing swallow is keyed on the event's Captured status, NOT
    // the search_input_focused mirror: after a mouse click away from the
    // search field the flag stays stale-true while real iced focus is gone,
    // and the keypress arrives Ignored — backward ring entry must work,
    // not be a dead key until something resets the flag.
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "precondition: the flag reads focused (stale)"
    );

    shift_backspace(&mut app); // Status::Ignored — nothing captured it

    assert_eq!(
        tray_cursor(&app),
        Some(TrawlTrayControl::MaxTracks),
        "an uncaptured Shift+Backspace enters the ring backward"
    );
}

#[test]
fn left_right_cycle_the_focused_value_with_wrap() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
        state.tray_cursor = Some(TrawlTrayControl::Blend);
    }
    assert_eq!(app.trawl_crate.blend, TrawlBlend::ALL[0]);

    arrow(&mut app, true);
    assert_eq!(
        app.trawl_crate.blend,
        TrawlBlend::ALL[1],
        "Right steps the focused control forward"
    );

    arrow(&mut app, false);
    arrow(&mut app, false);
    assert_eq!(
        app.trawl_crate.blend,
        TrawlBlend::ALL[TrawlBlend::ALL.len() - 1],
        "Left from the first value wraps to the last"
    );
}

#[test]
fn all_five_controls_cycle_their_own_value() {
    use nokkvi_data::types::trawl::{
        TrawlMaxLength, TrawlMaxTracks, TrawlMinLength, TrawlRatingFilter,
    };

    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
    }

    // Test-local re-derivation of "one step forward, wrapping" so the
    // assertion doesn't lean on the production helper it exists to check.
    fn next_of<T: Copy + PartialEq + std::fmt::Debug>(all: &[T], v: T) -> T {
        let i = all
            .iter()
            .position(|x| *x == v)
            .unwrap_or_else(|| panic!("{v:?} must be in its ALL array"));
        all[(i + 1) % all.len()]
    }

    // One Right press per control: exactly that control's crate field steps
    // to its ALL-neighbor — pins the per-variant field wiring.
    for control in TrawlTrayControl::ALL {
        if let Some(state) = app.trawl_modal.as_mut() {
            state.tray_cursor = Some(control);
        }
        let before = app.trawl_crate.clone();
        arrow(&mut app, true);
        let after = &app.trawl_crate;

        let stepped = |name: &str, changed: bool| {
            assert_eq!(
                changed,
                matches!(
                    (control, name),
                    (TrawlTrayControl::Blend, "blend")
                        | (TrawlTrayControl::MinLength, "min_length")
                        | (TrawlTrayControl::MaxLength, "max_length")
                        | (TrawlTrayControl::Rating, "rating")
                        | (TrawlTrayControl::MaxTracks, "max_tracks")
                ),
                "cursor {control:?}: only its own field may change, checked {name}"
            );
        };
        stepped("blend", after.blend != before.blend);
        stepped("min_length", after.min_length != before.min_length);
        stepped("max_length", after.max_length != before.max_length);
        stepped("rating", after.rating != before.rating);
        stepped("max_tracks", after.max_tracks != before.max_tracks);

        // ...and it landed exactly one wrapping step from where it was.
        match control {
            TrawlTrayControl::Blend => {
                assert_eq!(after.blend, next_of(&TrawlBlend::ALL, before.blend));
            }
            TrawlTrayControl::MinLength => {
                assert_eq!(
                    after.min_length,
                    next_of(&TrawlMinLength::ALL, before.min_length)
                );
            }
            TrawlTrayControl::MaxLength => {
                assert_eq!(
                    after.max_length,
                    next_of(&TrawlMaxLength::ALL, before.max_length)
                );
            }
            TrawlTrayControl::Rating => {
                assert_eq!(
                    after.rating,
                    next_of(&TrawlRatingFilter::ALL, before.rating)
                );
            }
            TrawlTrayControl::MaxTracks => {
                assert_eq!(
                    after.max_tracks,
                    next_of(&TrawlMaxTracks::ALL, before.max_tracks)
                );
            }
        }

        // Left returns to the starting value — catches a hardcoded direction
        // in any per-control arm (Right-then-Left must round-trip).
        arrow(&mut app, false);
        let reverted = &app.trawl_crate;
        assert_eq!(reverted.blend, before.blend, "{control:?}: Left reverts");
        assert_eq!(reverted.min_length, before.min_length);
        assert_eq!(reverted.max_length, before.max_length);
        assert_eq!(reverted.rating, before.rating);
        assert_eq!(reverted.max_tracks, before.max_tracks);
    }
}

#[test]
fn arrows_with_no_tray_cursor_are_inert() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
    }
    let before = app.trawl_crate.clone();

    arrow(&mut app, true);
    arrow(&mut app, false);

    assert_eq!(tray_cursor(&app), None, "no auto-enter on bare arrows");
    assert_eq!(app.trawl_crate.blend, before.blend);
    assert_eq!(app.trawl_crate.min_length, before.min_length);
    assert_eq!(app.trawl_crate.max_length, before.max_length);
    assert_eq!(app.trawl_crate.rating, before.rating);
    assert_eq!(app.trawl_crate.max_tracks, before.max_tracks);
}

#[test]
fn tray_keys_do_not_touch_the_background_view() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Songs);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
        state.tray_cursor = Some(TrawlTrayControl::Blend);
    }
    // The falsifiable observables are the SYNCHRONOUS side effects of
    // handle_cycle_sort_mode's pre-branch lines: reveal_current_toolbar()
    // sets toolbar_reveal_until and the standard-view arm clears the page's
    // search_input_focused. (The sort mutation itself rides a Task tests
    // never run — asserting it alone would be green-by-construction.)
    app.songs_page.common.search_input_focused = true;
    let sort_before = app.songs_page.common.current_sort_mode;

    arrow(&mut app, true);
    shift_tab(&mut app);

    assert_eq!(
        app.songs_page.common.current_sort_mode, sort_before,
        "the obscured view's sort mode must not cycle"
    );
    assert!(
        app.songs_page.common.toolbar_reveal_until.is_none(),
        "no stray auto-hide toolbar reveal-lock may be stranded on the \
         obscured view (the branch must run before reveal_current_toolbar)"
    );
    assert!(
        app.songs_page.common.search_input_focused,
        "the obscured view's search-focus flag must not be cleared \
         (the standard-view sort arm must be unreachable)"
    );
}

#[test]
fn tray_keys_do_not_cycle_queue_sort() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
        state.tray_cursor = Some(TrawlTrayControl::MinLength);
    }
    // Same falsifiability note as tray_keys_do_not_touch_the_background_view:
    // the Queue sort mutation rides a discarded Task, so the real pins are
    // the synchronous reveal-lock and the Queue arm's focus-flag clear.
    app.queue_page.common.search_input_focused = true;
    let mode_before = app.queue_page.queue_sort_mode;
    let sorted_before = app.queue_page.queue_sorted;

    arrow(&mut app, true);

    assert_eq!(app.queue_page.queue_sort_mode, mode_before);
    assert_eq!(
        app.queue_page.queue_sorted, sorted_before,
        "Left/Right must edit the tray, never the queue sort underneath"
    );
    assert!(
        app.queue_page.common.toolbar_reveal_until.is_none(),
        "no reveal-lock may be stranded on the obscured queue"
    );
    assert!(
        app.queue_page.common.search_input_focused,
        "the queue sort arm's focus-flag clear must be unreachable"
    );
}

#[test]
fn newly_admitted_keys_stay_swallowed_over_other_modals() {
    // The CycleSortMode / SettingsCategoryMotion admissions live inside the
    // trawl-gated is_trawl_nav arm — the EQ/Info/About modals must keep
    // swallowing the same keys.
    let mut app = test_app();
    app.current_view = crate::View::Settings;
    app.screen = crate::Screen::Home;
    app.eq_modal.open = true;
    let sidebar_before = app.settings_page.sidebar_slot_list.viewport_offset;
    shift_tab(&mut app);
    assert_eq!(
        app.settings_page.sidebar_slot_list.viewport_offset, sidebar_before,
        "Shift+Tab over the EQ modal must not move the settings sidebar"
    );

    let mut app = test_app();
    app.current_view = crate::View::Songs;
    app.screen = crate::Screen::Home;
    app.info_modal.visible = true;
    arrow(&mut app, true);
    assert!(
        app.songs_page.common.toolbar_reveal_until.is_none(),
        "Right over the info modal must not reach the sort-cycle handler"
    );
}

#[test]
fn shift_tab_while_modal_open_does_not_move_settings_sidebar() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    let sidebar_before = app.settings_page.sidebar_slot_list.viewport_offset;

    shift_tab(&mut app);

    assert_eq!(
        app.settings_page.sidebar_slot_list.viewport_offset, sidebar_before,
        "category motion must route to the tray, not the hidden settings sidebar"
    );
}

#[test]
fn cycle_sort_without_modal_still_reveals_toolbar() {
    // Over-match guard: the new trawl-first branch must not swallow the
    // regular sort-cycle path when no modal is open.
    let mut app = test_app();
    app.current_view = crate::View::Songs;
    app.screen = crate::Screen::Home;

    arrow(&mut app, true);

    assert!(
        app.songs_page.common.toolbar_reveal_until.is_some(),
        "without the modal, Left/Right still drive the view's sort cycle"
    );
}

#[test]
fn settings_category_motion_without_modal_still_moves_sidebar() {
    // Over-match guard for the other handler: Settings keeps its sidebar nav.
    let mut app = test_app();
    app.current_view = crate::View::Settings;
    app.screen = crate::Screen::Home;
    let before = app.settings_page.sidebar_slot_list.viewport_offset;

    shift_tab(&mut app);

    assert_ne!(
        app.settings_page.sidebar_slot_list.viewport_offset, before,
        "Shift+Tab in Settings must still move the sidebar category"
    );
}

#[test]
fn slash_clears_the_tray_cursor_when_refocusing_search() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
        state.tray_cursor = Some(TrawlTrayControl::Rating);
    }

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("/".into()),
        iced::keyboard::Modifiers::empty(),
    );

    assert_eq!(
        tray_cursor(&app),
        None,
        "the ring must never show while the search field owns the arrows"
    );
    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused)
    );
}

#[test]
fn slash_inside_modal_does_not_reveal_background_toolbar() {
    // Same regression class the tray branches guard against: `/` with the
    // modal open must not strand an auto-hide reveal-lock on the obscured
    // view — its target is the modal's own always-rendered search field.
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Songs);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
    }

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("/".into()),
        iced::keyboard::Modifiers::empty(),
    );

    assert!(
        app.trawl_modal
            .as_ref()
            .is_some_and(|s| s.search_input_focused),
        "/ still refocuses the modal search"
    );
    assert!(
        app.songs_page.common.toolbar_reveal_until.is_none(),
        "no reveal-lock may be stranded on the obscured view"
    );
}

#[test]
fn typing_in_search_clears_the_tray_cursor() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.tray_cursor = Some(TrawlTrayControl::MaxTracks);
    }

    let _ = app.handle_trawl_modal(TrawlModalMessage::SearchChanged("bu".into()));

    assert_eq!(
        tray_cursor(&app),
        None,
        "typing hands the keys to the field"
    );
}

#[test]
fn reopen_resets_the_tray_cursor() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    if let Some(state) = app.trawl_modal.as_mut() {
        state.tray_cursor = Some(TrawlTrayControl::MaxLength);
    }

    let _ = app.handle_trawl_modal(TrawlModalMessage::Close);
    open_modal(&mut app);

    assert_eq!(
        tray_cursor(&app),
        None,
        "a fresh modal starts with search focused and no tray ring"
    );
}

#[test]
fn escape_with_tray_cursor_active_is_not_two_stage() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    app.trawl_crate.add(seed("al1"));
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_input_focused = false;
        state.tray_cursor = Some(TrawlTrayControl::Blend);
    }

    // Escape's close rides a Task tests never run, so modal closure itself
    // is pinned by escape_closes_the_trawl_modal_and_the_crate_survives.
    // What THIS test pins is the design decision that Escape is NOT
    // two-stage: no synchronous first-press ring clear may creep in (the
    // ring's None position is the in-modal dismiss; Escape always closes).
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        iced::keyboard::Modifiers::empty(),
    );
    assert_eq!(
        tray_cursor(&app),
        Some(TrawlTrayControl::Blend),
        "Escape must not clear the ring as a first stage"
    );

    let _ = app.update(crate::Message::TrawlModal(TrawlModalMessage::Close));
    assert!(app.trawl_modal.is_none(), "the emitted Close still closes");
    assert_eq!(app.trawl_crate.len(), 1, "the crate persists");
}

#[test]
fn enter_toggles_seed_with_tray_cursor_active() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    app.trawl_search_generation = 1;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_query = "ph".into();
        state.search_results = Some(*results_with_genre());
        state.slot_list.set_selected(1, 2);
        state.search_input_focused = false;
        state.tray_cursor = Some(TrawlTrayControl::MinLength);
    }

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::empty(),
    );

    assert!(
        app.trawl_crate
            .contains(&BatchItem::Genre("Phonk".to_string())),
        "Enter keeps seeding the centered row — the tray ring never captures it"
    );
    assert_eq!(
        tray_cursor(&app),
        Some(TrawlTrayControl::MinLength),
        "seeding leaves the tray cursor in place"
    );
}

#[test]
fn list_nav_keeps_the_tray_cursor() {
    let mut app = test_app();
    open_modal_over(&mut app, crate::View::Queue);
    app.trawl_search_generation = 1;
    if let Some(state) = app.trawl_modal.as_mut() {
        state.search_query = "ph".into();
        state.search_results = Some(*results_with_genre());
        state.search_input_focused = false;
        state.tray_cursor = Some(TrawlTrayControl::Rating);
    }

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
        iced::keyboard::Modifiers::empty(),
    );
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
        iced::keyboard::Modifiers::empty(),
    );

    assert_eq!(
        tray_cursor(&app),
        Some(TrawlTrayControl::Rating),
        "list nav and the tray ring are disjoint key axes — no re-entry tax"
    );
}

// ---- Shift+A (AddToQueue) = add the mix to the queue -----------------------

#[test]
fn shift_a_in_modal_routes_to_add_mix_empty_crate_warns() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);

    // Shift+A resolves to AddToQueue; inside the modal the one enqueueable
    // thing is the mix. Empty crate → the warn toast is the observable proof
    // the key was admitted + routed rather than swallowed by the modal gate.
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("A".into()),
        iced::keyboard::Modifiers::SHIFT,
    );

    assert!(
        !app.toast.toasts.is_empty(),
        "Shift+A must reach the trawl AddMix route (empty-crate warn), not be swallowed"
    );
}

#[test]
fn add_to_queue_hotkey_routes_to_the_mix_not_the_obscured_view() {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));

    let _ = app.handle_add_to_queue();

    assert!(
        app.toast.toasts.is_empty(),
        "a seeded crate routes to AddMixToQueue — not the obscured view's \
         fallback ('No item selected' toast)"
    );
}

#[test]
fn rebound_add_to_queue_captured_mid_typing_does_not_double_fire() {
    use nokkvi_data::types::hotkey_config::{HotkeyAction, KeyCode, KeyCombo};

    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    // Rebind AddToQueue to Shift+Backspace — a combo the focused search field
    // captures (it deletes a character). One press, one meaning: the captured
    // event must not ALSO drive the mix-add (whose empty-crate warn toast
    // would betray the double-handling).
    app.hotkey_config.set_binding(
        HotkeyAction::AddToQueue,
        KeyCombo::shift(KeyCode::Backspace),
    );

    let _ = send_raw_key_captured(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
        iced::keyboard::Modifiers::SHIFT,
    );

    assert!(
        app.toast.toasts.is_empty(),
        "captured Shift+Backspace already edited the field; it must not also fire AddMix"
    );
}

// ---- M7: Save Mix as Playlist ----------------------------------------------

#[test]
fn save_as_playlist_with_empty_crate_warns() {
    let mut app = test_app();
    open_modal(&mut app);
    let _ = app.handle_trawl_modal(TrawlModalMessage::SaveAsPlaylist);
    let toast = app.toast.toasts.back().expect("empty-crate warn");
    assert!(toast.message.contains("crate is empty"));
    assert!(app.trawl_modal.is_some(), "modal stays open");
}

#[test]
fn save_resolve_ok_closes_modal_and_opens_name_dialog() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));

    let _ = app.handle_trawl_modal(TrawlModalMessage::SaveResolveCompleted(Ok(vec![
        "s1".into(),
        "s2".into(),
        "s3".into(),
    ])));

    assert!(app.trawl_modal.is_none(), "modal hands off to the dialog");
    assert!(app.text_input_dialog.visible);
    assert_eq!(app.text_input_dialog.title, "Save Mix as Playlist");
    match &app.text_input_dialog.action {
        Some(
            crate::widgets::text_input_dialog::TextInputDialogAction::CreatePlaylistFromTrawl(ids),
        ) => assert_eq!(ids.len(), 3, "resolved ids ride the action"),
        other => panic!("expected CreatePlaylistFromTrawl, got {other:?}"),
    }
    assert_eq!(
        app.text_input_dialog.note.as_deref(),
        Some("Saves these 3 songs as an ordinary playlist — it won't update"),
        "the honesty subtitle, verbatim"
    );
}

#[test]
fn save_resolve_err_keeps_modal_and_toasts() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));

    let _ = app.handle_trawl_modal(TrawlModalMessage::SaveResolveCompleted(Err(
        "every song was filtered out".into(),
    )));

    assert!(
        app.trawl_modal.is_some(),
        "failure is actionable in the modal"
    );
    assert!(!app.text_input_dialog.visible);
    let toast = app.toast.toasts.back().expect("error toast");
    assert!(toast.message.contains("Failed to resolve mix"));
}

#[test]
fn name_dialog_cancel_reopens_trawl_modal() {
    let mut app = test_app();
    open_modal(&mut app);
    app.trawl_crate.add(seed("al1"));
    let _ = app.handle_trawl_modal(TrawlModalMessage::SaveResolveCompleted(Ok(vec![
        "s1".into(),
    ])));
    assert!(app.trawl_modal.is_none());

    let _ = app.update(crate::Message::TextInputDialog(
        crate::widgets::text_input_dialog::TextInputDialogMessage::Cancel,
    ));

    assert!(!app.text_input_dialog.visible);
    assert!(
        app.trawl_modal.is_some(),
        "cancel backs out of NAMING, not out of the mix"
    );
    assert_eq!(app.trawl_crate.len(), 1, "crate untouched");
}

#[test]
fn shift_p_routes_to_save_only_inside_the_modal() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;

    // Modal closed: quiet no-op (no dialog, no toast).
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("p".into()),
        iced::keyboard::Modifiers::SHIFT,
    );
    assert!(!app.text_input_dialog.visible);
    assert!(
        app.toast.toasts.is_empty(),
        "closed-modal Shift+P stays quiet"
    );

    // Modal open, empty crate: routes trawl-first (the explanatory warn).
    open_modal(&mut app);
    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("p".into()),
        iced::keyboard::Modifiers::SHIFT,
    );
    let toast = app.toast.toasts.back().expect("empty-crate warn");
    assert!(toast.message.contains("crate is empty"));
}

#[test]
fn captured_shift_p_mid_typing_does_not_double_fire() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    open_modal(&mut app);
    // A captured Shift+P already typed "P" into the search field — it must
    // not ALSO trigger the save (whose empty-crate warn would betray the
    // double-handling).
    let _ = send_raw_key_captured(
        &mut app,
        iced::keyboard::Key::Character("p".into()),
        iced::keyboard::Modifiers::SHIFT,
    );
    assert!(
        app.toast.toasts.is_empty(),
        "captured Shift+P already typed; it must not also fire the save"
    );
}
