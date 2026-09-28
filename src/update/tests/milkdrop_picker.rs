//! The MilkDrop preset picker: opening over a running preset, previewing the
//! centered row live, choosing (and locking) it, and closing back to the
//! preset that was on screen. The GPU half is out of reach here (see
//! `milkdrop.rs`); these assert the load bookkeeping the picker drives.

use nokkvi_data::types::player_settings::VisualizationMode;

use super::milkdrop::{built, first_frame, md_app, on_screen, past, press_plain, tick};
use crate::{
    Nokkvi,
    app_message::{Message, MilkdropControl, MilkdropMessage, SlotListMessage},
    widgets::milkdrop_picker::MilkdropPickerMessage as P,
};

fn open(app: &mut Nokkvi) {
    let _ = app.update(Message::Milkdrop(MilkdropMessage::Control(
        MilkdropControl::ChoosePreset,
    )));
}

fn picker(app: &mut Nokkvi, msg: P) {
    let _ = app.update(Message::MilkdropPicker(msg));
}

fn centered(app: &Nokkvi) -> Option<String> {
    app.milkdrop
        .picker
        .as_ref()
        .and_then(|p| p.centered().map(str::to_string))
}

fn key(app: &mut Nokkvi, named: iced::keyboard::key::Named) {
    let _ = app.handle_raw_key_event(
        iced::keyboard::Key::Named(named),
        iced::keyboard::Modifiers::default(),
        iced::event::Status::Ignored,
    );
}

/// A running app with a preset on screen and the picker open over it. The
/// preset on screen is a fixed mid-list one, so a step either way moves off it
/// (the first draw is random and could be the last row).
fn open_over_running() -> (Nokkvi, String) {
    let mut app = md_app();
    on_screen(&mut app);
    let library = &app.milkdrop.library;
    let original = library.entries()[library.len() / 2].name.clone();
    let _ = app.milkdrop_load(original.clone());
    land(&mut app);
    assert_eq!(app.milkdrop.on_screen.as_deref(), Some(original.as_str()));
    open(&mut app);
    assert!(app.milkdrop.picker.is_some(), "picker opened");
    (app, original)
}

/// Land the build in flight and its first frame.
fn land(app: &mut Nokkvi) {
    let generation = app.milkdrop.generation;
    built(app, generation, Ok(()));
    first_frame(app);
}

// ----------------------------------------------------------------------------
// Opening
// ----------------------------------------------------------------------------

#[test]
fn opens_centered_on_the_preset_on_screen_and_holds_the_lock() {
    let (app, original) = open_over_running();
    assert_eq!(centered(&app).as_deref(), Some(original.as_str()));
    assert!(
        app.milkdrop.locked,
        "the timer cannot swap presets mid-pick"
    );
    assert_eq!(app.milkdrop.next_switch_at, None);
    let state = app.milkdrop.picker.as_ref().expect("open");
    assert!(!state.was_locked);
    assert_eq!(state.original.as_deref(), Some(original.as_str()));
}

#[test]
fn the_m_key_opens_the_picker() {
    let mut app = md_app();
    on_screen(&mut app);
    press_plain(&mut app, "m");
    assert!(app.milkdrop.picker.is_some());
}

#[test]
fn does_not_open_outside_milkdrop_mode() {
    let mut app = md_app();
    app.engine.visualization_mode = VisualizationMode::Bars;
    let toasts = app.toast.toasts.len();
    open(&mut app);
    assert!(app.milkdrop.picker.is_none());
    assert_eq!(app.toast.toasts.len(), toasts + 1, "says why");
}

#[test]
fn opens_while_paused() {
    let mut app = md_app();
    on_screen(&mut app);
    app.playback.paused = true;
    open(&mut app);
    assert!(app.milkdrop.picker.is_some());
}

// ----------------------------------------------------------------------------
// Live preview
// ----------------------------------------------------------------------------

#[test]
fn moving_the_list_previews_the_centered_preset() {
    let (mut app, original) = open_over_running();
    let generation = app.milkdrop.generation;
    picker(&mut app, P::SlotListDown);
    let target = centered(&app).expect("a row");
    assert_ne!(target, original);
    assert_eq!(app.milkdrop.current.as_deref(), Some(target.as_str()));
    assert_eq!(app.milkdrop.generation, generation + 1);
    assert!(
        app.milkdrop.history.is_empty(),
        "previews never enter Previous's history"
    );
}

#[test]
fn preview_waits_for_the_build_in_flight_then_catches_up() {
    let (mut app, _) = open_over_running();
    picker(&mut app, P::SlotListDown);
    let first = app.milkdrop.current.clone();
    let generation = app.milkdrop.generation;
    // Two more steps while the first preview still builds: nothing new loads.
    picker(&mut app, P::SlotListDown);
    picker(&mut app, P::SlotListDown);
    assert_eq!(app.milkdrop.generation, generation, "one build at a time");
    assert_eq!(app.milkdrop.current, first);

    // The build lands; the next tick loads where the list is now.
    built(&mut app, generation, Ok(()));
    tick(&mut app);
    assert_eq!(app.milkdrop.current, centered(&app));
    assert_eq!(app.milkdrop.generation, generation + 1);
}

#[test]
fn preview_is_idle_while_paused() {
    let (mut app, original) = open_over_running();
    app.playback.paused = true;
    picker(&mut app, P::SlotListDown);
    tick(&mut app);
    assert_eq!(app.milkdrop.current.as_deref(), Some(original.as_str()));
}

#[test]
fn preview_toasts_no_names() {
    let (mut app, _) = open_over_running();
    let toasts = app.toast.toasts.len();
    picker(&mut app, P::SlotListDown);
    land(&mut app);
    assert!(app.milkdrop.on_screen.is_some());
    assert_eq!(app.toast.toasts.len(), toasts, "the modal already shows it");
}

#[test]
fn the_timer_and_track_changes_leave_an_open_picker_alone() {
    let (mut app, original) = open_over_running();
    app.milkdrop.next_switch_at = Some(past());
    tick(&mut app);
    assert_eq!(app.milkdrop.current.as_deref(), Some(original.as_str()));
    let _ = app.milkdrop_on_track_change();
    assert_eq!(app.milkdrop.current.as_deref(), Some(original.as_str()));
}

#[test]
fn a_failed_preview_does_not_jump_to_a_random_preset() {
    let (mut app, _) = open_over_running();
    picker(&mut app, P::SlotListDown);
    let broken = centered(&app).expect("a row");
    let generation = app.milkdrop.generation;
    built(&mut app, generation, Err("bad shader".into()));
    tick(&mut app);
    assert!(app.milkdrop.library.is_broken(&broken));
    assert_eq!(
        app.milkdrop.generation, generation,
        "nothing else loads while the picker sits on the broken row"
    );
}

// ----------------------------------------------------------------------------
// Choosing
// ----------------------------------------------------------------------------

#[test]
fn enter_keeps_the_preview_locks_it_and_records_the_original() {
    let (mut app, original) = open_over_running();
    picker(&mut app, P::SlotListDown);
    let target = centered(&app).expect("a row");
    land(&mut app);
    picker(&mut app, P::ActivateCenter);
    assert!(app.milkdrop.picker.is_none());
    assert_eq!(app.milkdrop.current.as_deref(), Some(target.as_str()));
    assert!(app.milkdrop.locked, "a chosen preset stays");
    assert_eq!(app.milkdrop.history, std::slice::from_ref(&original));
}

#[test]
fn choosing_while_paused_loads_the_choice() {
    let (mut app, _) = open_over_running();
    app.playback.paused = true;
    picker(&mut app, P::SlotListDown);
    let target = centered(&app).expect("a row");
    picker(&mut app, P::ActivateCenter);
    assert_eq!(app.milkdrop.current.as_deref(), Some(target.as_str()));
    assert!(app.milkdrop.locked);
}

#[test]
fn clicking_a_row_chooses_it() {
    let (mut app, _) = open_over_running();
    let target = app.milkdrop.picker.as_ref().expect("open").filtered[3].clone();
    picker(&mut app, P::ClickItem(3));
    assert!(app.milkdrop.picker.is_none());
    assert_eq!(app.milkdrop.current.as_deref(), Some(target.as_str()));
    assert!(app.milkdrop.locked);
}

#[test]
fn choosing_the_original_again_keeps_it_running() {
    let (mut app, original) = open_over_running();
    let generation = app.milkdrop.generation;
    picker(&mut app, P::ActivateCenter);
    assert_eq!(app.milkdrop.current.as_deref(), Some(original.as_str()));
    assert_eq!(app.milkdrop.generation, generation, "no restart from black");
    assert!(app.milkdrop.locked);
    assert!(app.milkdrop.history.is_empty());
}

#[test]
fn a_broken_row_cannot_be_chosen() {
    let (mut app, _) = open_over_running();
    picker(&mut app, P::SlotListDown);
    let broken = centered(&app).expect("a row");
    app.milkdrop.library.mark_broken(&broken);
    let toasts = app.toast.toasts.len();
    picker(&mut app, P::ActivateCenter);
    assert!(app.milkdrop.picker.is_some(), "stays open");
    assert_eq!(app.toast.toasts.len(), toasts + 1, "says why");
}

// ----------------------------------------------------------------------------
// Closing without choosing
// ----------------------------------------------------------------------------

#[test]
fn close_returns_to_the_original_and_restores_the_lock() {
    let (mut app, original) = open_over_running();
    picker(&mut app, P::SlotListDown);
    land(&mut app);
    picker(&mut app, P::Close);
    assert!(app.milkdrop.picker.is_none());
    assert_eq!(app.milkdrop.current.as_deref(), Some(original.as_str()));
    assert!(!app.milkdrop.locked);
    assert!(app.milkdrop.history.is_empty());
}

#[test]
fn close_restores_a_lock_that_was_on() {
    let mut app = md_app();
    on_screen(&mut app);
    app.milkdrop.locked = true;
    open(&mut app);
    picker(&mut app, P::Close);
    assert!(app.milkdrop.locked);
}

#[test]
fn close_without_previewing_reloads_nothing() {
    let (mut app, _) = open_over_running();
    let generation = app.milkdrop.generation;
    picker(&mut app, P::Close);
    assert_eq!(app.milkdrop.generation, generation);
}

#[test]
fn close_after_hiding_the_original_moves_on() {
    let (mut app, original) = open_over_running();
    picker(&mut app, P::ToggleHidden(original.clone()));
    picker(&mut app, P::SlotListDown);
    picker(&mut app, P::Close);
    tick(&mut app);
    assert_ne!(
        app.milkdrop.current.as_deref(),
        Some(original.as_str()),
        "a hidden preset is never brought back"
    );
}

#[test]
fn escape_closes_the_picker() {
    let (mut app, original) = open_over_running();
    picker(&mut app, P::SlotListDown);
    key(&mut app, iced::keyboard::key::Named::Escape);
    assert!(app.milkdrop.picker.is_none());
    assert_eq!(app.milkdrop.current.as_deref(), Some(original.as_str()));
}

#[test]
fn leaving_milkdrop_mode_closes_the_picker() {
    let (mut app, _) = open_over_running();
    let _ = app.update(Message::Playback(
        crate::app_message::PlaybackMessage::CycleVisualization,
    ));
    assert_ne!(app.engine.visualization_mode, VisualizationMode::Milkdrop);
    assert!(app.milkdrop.picker.is_none());
    assert!(!app.milkdrop.locked, "the picker's hold is let go");
}

// ----------------------------------------------------------------------------
// Keyboard ownership
// ----------------------------------------------------------------------------

#[test]
fn bare_keys_do_not_reach_the_view_behind() {
    let (mut app, original) = open_over_running();
    let random = app.modes.random;
    press_plain(&mut app, "x");
    press_plain(&mut app, "n");
    assert_eq!(app.modes.random, random, "ToggleRandom swallowed");
    assert_eq!(
        app.milkdrop.current.as_deref(),
        Some(original.as_str()),
        "Next Preset swallowed"
    );
}

#[test]
fn slot_list_keys_steer_the_picker() {
    let (mut app, original) = open_over_running();
    let _ = app.update(Message::SlotList(SlotListMessage::NavigateDown));
    let target = centered(&app).expect("a row");
    assert_ne!(target, original);
    let _ = app.update(Message::SlotList(SlotListMessage::ActivateCenter));
    assert!(app.milkdrop.picker.is_none());
    assert_eq!(app.milkdrop.current.as_deref(), Some(target.as_str()));
}

// ----------------------------------------------------------------------------
// Curation from the rows
// ----------------------------------------------------------------------------

#[test]
fn the_heart_toggles_a_rows_favorite() {
    let (mut app, _) = open_over_running();
    let name = app.milkdrop.picker.as_ref().expect("open").filtered[2].clone();
    picker(&mut app, P::ToggleFavorite(name.clone()));
    assert!(app.milkdrop.library.is_favorite(&name));
    assert!(app.milkdrop.picker.is_some(), "stays open");
    picker(&mut app, P::ToggleFavorite(name.clone()));
    assert!(!app.milkdrop.library.is_favorite(&name));
}

#[test]
fn the_eye_hides_and_unhides() {
    let (mut app, _) = open_over_running();
    let name = app.milkdrop.picker.as_ref().expect("open").filtered[2].clone();
    app.milkdrop.history.push(name.clone());
    picker(&mut app, P::ToggleHidden(name.clone()));
    assert!(app.milkdrop.library.is_hidden(&name));
    assert!(
        !app.milkdrop.history.contains(&name),
        "a hidden preset leaves Previous's history"
    );
    picker(&mut app, P::ToggleHidden(name.clone()));
    assert!(!app.milkdrop.library.is_hidden(&name));
}

#[test]
fn a_hidden_preset_can_still_be_chosen() {
    let (mut app, _) = open_over_running();
    picker(&mut app, P::SlotListDown);
    let target = centered(&app).expect("a row");
    picker(&mut app, P::ToggleHidden(target.clone()));
    picker(&mut app, P::ActivateCenter);
    assert_eq!(app.milkdrop.current.as_deref(), Some(target.as_str()));
    assert!(
        app.milkdrop.library.is_hidden(&target),
        "still out of rotation"
    );
}

#[test]
fn favorites_chip_filters_and_unfavoriting_drops_the_row() {
    let (mut app, _) = open_over_running();
    let names: Vec<String> = app.milkdrop.picker.as_ref().expect("open").filtered[..2].to_vec();
    for name in &names {
        picker(&mut app, P::ToggleFavorite(name.clone()));
    }
    picker(&mut app, P::ToggleFavoritesOnly);
    assert_eq!(app.milkdrop.picker.as_ref().expect("open").filtered, names);
    picker(&mut app, P::ToggleFavorite(names[0].clone()));
    assert_eq!(
        app.milkdrop.picker.as_ref().expect("open").filtered,
        [names[1].clone()]
    );
}

#[test]
fn search_filters_and_previews_the_first_match() {
    let (mut app, _) = open_over_running();
    picker(&mut app, P::SearchChanged("fjord".into()));
    let state = app.milkdrop.picker.as_ref().expect("open");
    assert!(
        state
            .filtered
            .iter()
            .all(|n| n.to_lowercase().contains("fjord"))
    );
    assert!(!state.filtered.is_empty());
    assert_eq!(app.milkdrop.current, centered(&app));
}

#[test]
fn the_panel_menu_row_opens_the_picker() {
    let mut app = md_app();
    on_screen(&mut app);
    let _ = app.update(Message::Queue(crate::views::QueueMessage::Milkdrop(
        MilkdropControl::ChoosePreset,
    )));
    assert!(app.milkdrop.picker.is_some());
}

#[test]
fn arrow_keys_step_the_picker() {
    let (mut app, original) = open_over_running();
    key(&mut app, iced::keyboard::key::Named::ArrowDown);
    let down = centered(&app).expect("a row");
    assert_ne!(down, original);
    key(&mut app, iced::keyboard::key::Named::ArrowUp);
    assert_eq!(centered(&app).as_deref(), Some(original.as_str()));
}

#[test]
fn escape_in_theater_closes_the_picker_not_theater() {
    let (mut app, _) = open_over_running();
    app.theater.active = true;
    key(&mut app, iced::keyboard::key::Named::Escape);
    assert!(app.milkdrop.picker.is_none());
    assert!(app.theater.active, "one Escape, one layer");
}

// ----------------------------------------------------------------------------
// Review fixes
// ----------------------------------------------------------------------------

#[test]
fn the_preset_verb_is_refused_while_the_picker_is_open() {
    use crate::update::milkdrop::PresetAction;
    let (mut app, _) = open_over_running();
    picker(&mut app, P::SlotListDown);
    land(&mut app);
    let current = app.milkdrop.current.clone();
    for action in [
        PresetAction::Next,
        PresetAction::Previous,
        PresetAction::Lock,
        PresetAction::Unlock,
        PresetAction::Hide,
    ] {
        assert!(app.milkdrop_ipc_control(action).is_err(), "{action:?}");
    }
    assert_eq!(app.milkdrop.current, current);
    assert!(app.milkdrop.history.is_empty());
    assert!(app.milkdrop.locked, "the picker's hold stands");
}

#[test]
fn close_after_hiding_the_original_in_place_moves_on() {
    let (mut app, original) = open_over_running();
    picker(&mut app, P::ToggleHidden(original.clone()));
    picker(&mut app, P::Close);
    tick(&mut app);
    assert_ne!(
        app.milkdrop.current.as_deref(),
        Some(original.as_str()),
        "the preset just hidden does not stay on screen"
    );
}

#[test]
fn a_chosen_preset_that_fails_releases_the_lock() {
    let (mut app, _) = open_over_running();
    picker(&mut app, P::SlotListDown);
    picker(&mut app, P::ActivateCenter);
    assert!(app.milkdrop.locked);
    let generation = app.milkdrop.generation;
    let toasts = app.toast.toasts.len();
    built(&mut app, generation, Err("bad shader".into()));
    assert!(!app.milkdrop.locked, "the random replacement is not held");
    assert!(app.toast.toasts.len() > toasts, "says why");
}

#[test]
fn a_chosen_preset_that_lands_keeps_the_lock() {
    let (mut app, _) = open_over_running();
    picker(&mut app, P::SlotListDown);
    picker(&mut app, P::ActivateCenter);
    land(&mut app);
    assert_eq!(app.milkdrop.chosen_generation, None);
    // A later failure (say, a theme recolour) is not the choice's.
    let _ = app.milkdrop_load(app.milkdrop.current.clone().expect("current"));
    let generation = app.milkdrop.generation;
    built(&mut app, generation, Err("bad shader".into()));
    assert!(app.milkdrop.locked);
}

#[test]
fn opening_the_picker_cancels_a_roulette_spin() {
    let (mut app, _) = open_over_running();
    picker(&mut app, P::Close);
    app.library.albums.set_from_vec(
        (0..8)
            .map(|i| crate::test_helpers::make_album(&format!("a{i}"), "Album", "Artist"))
            .collect(),
    );
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(
        crate::View::Albums,
    ));
    assert!(app.roulette.is_some());
    open(&mut app);
    assert!(
        app.roulette.is_none(),
        "Enter and Escape belong to the picker"
    );
    assert!(app.milkdrop.picker.is_some());
}
