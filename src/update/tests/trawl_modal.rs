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
    widgets::trawl_modal::{TrawlModalMessage, TrawlModalState},
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

    let _ = app.handle_trawl_modal(TrawlModalMessage::SetMinRating(
        nokkvi_data::types::trawl::TrawlMinRating::R4,
    ));
    assert_eq!(
        app.trawl_crate.min_rating,
        nokkvi_data::types::trawl::TrawlMinRating::R4
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
