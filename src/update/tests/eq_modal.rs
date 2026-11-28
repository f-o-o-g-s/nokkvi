//! The equalizer modal: the view switch, the Graphic sliders' save, the
//! Parametric band list edits, the typed fields, import, saved profiles and
//! the modal's teardown.
//!
//! Assertions target observable state only: `app.playback.eq_state` (the one
//! runtime copy of the equalizer) and `app.eq_modal` (`test_app()` has no
//! `AppService`, so the persists are dropped).

use nokkvi_data::audio::eq::{
    EQ_DEFAULT_Q, EQ_FREQ_MAX, EQ_MAX_BANDS, EqBand, EqBandKind, EqMode, EqProfile, graphic_bands,
};

use crate::{
    Nokkvi,
    test_helpers::test_app,
    update::modals::ActiveModal,
    widgets::{
        EqModalMessage as M,
        eq_parametric::{BandField, FieldKey, field_id},
    },
};

fn peak(freq: f64, gain: f64, q: f64) -> EqBand {
    EqBand::new(EqBandKind::Peak, freq, gain, q)
}

/// An app with the EQ modal open on the Parametric view over `bands`.
fn parametric_app(bands: &[EqBand]) -> Nokkvi {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = crate::View::Queue;
    app.playback.eq_state.set_mode(EqMode::Parametric);
    app.playback.eq_state.set_parametric_bands(bands);
    let _ = app.handle_eq_modal(M::Open);
    app
}

fn bands(app: &Nokkvi) -> Vec<EqBand> {
    app.playback.eq_state.parametric_bands().to_vec()
}

fn three_bands() -> [EqBand; 3] {
    [
        peak(100.0, 3.0, 1.0),
        peak(1_000.0, -2.0, 1.41),
        EqBand::new(EqBandKind::HighShelf, 8_000.0, 4.0, EQ_DEFAULT_Q),
    ]
}

// ----------------------------------------------------------------------------
// The view switch
// ----------------------------------------------------------------------------

#[test]
fn the_view_switch_keeps_both_layouts_and_the_master_switch() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Graphic));
    let gains = [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -4.0];
    app.playback.eq_state.set_all_gains(&gains);
    assert!(!app.playback.eq_state.is_enabled());

    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Parametric));
    assert_eq!(app.playback.eq_state.mode(), EqMode::Parametric);
    assert!(
        !app.playback.eq_state.is_enabled(),
        "switching the layout leaves the master switch alone"
    );
    assert_eq!(app.playback.eq_state.graphic_gains(), gains);
    assert_eq!(bands(&app), three_bands());

    let _ = app.handle_eq_modal(M::ToggleEnabled);
    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Graphic));
    assert_eq!(app.playback.eq_state.mode(), EqMode::Graphic);
    assert!(app.playback.eq_state.is_enabled(), "and stays on");
    assert_eq!(bands(&app), three_bands(), "the band list waits");
}

#[test]
fn switching_to_parametric_drops_the_preset_name_prompt() {
    let mut app = parametric_app(&[]);
    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Graphic));
    let _ = app.handle_eq_modal(M::SavePreset);
    let _ = app.handle_eq_modal(M::SavePresetNameChanged("Mine".into()));
    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Parametric));
    assert!(!app.eq_modal.save_mode);
    assert!(app.eq_modal.save_name.is_empty());
}

// ----------------------------------------------------------------------------
// Adding and removing bands
// ----------------------------------------------------------------------------

#[test]
fn add_band_appends_a_1_khz_peak_and_selects_it() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::AddBand);
    let list = bands(&app);
    assert_eq!(list.len(), 4);
    assert_eq!(list[3], peak(1_000.0, 0.0, 1.0));
    assert_eq!(app.eq_modal.selected, Some(3));
}

#[test]
fn a_double_click_adds_a_peak_where_it_landed() {
    let mut app = parametric_app(&[]);
    let _ = app.handle_eq_modal(M::AddBandAt(250.0, -4.5));
    assert_eq!(bands(&app), [peak(250.0, -4.5, 1.0)]);
    assert_eq!(app.eq_modal.selected, Some(0));
}

#[test]
fn the_band_list_stops_at_its_cap_with_a_toast() {
    let full: Vec<EqBand> = (0..EQ_MAX_BANDS)
        .map(|i| peak(100.0 + i as f64, 0.0, 1.0))
        .collect();
    let mut app = parametric_app(&full);
    let toasts = app.toast.toasts.len();
    let _ = app.handle_eq_modal(M::AddBand);
    let _ = app.handle_eq_modal(M::AddBandAt(500.0, 3.0));
    assert_eq!(bands(&app).len(), EQ_MAX_BANDS, "no 33rd band");
    assert_eq!(app.toast.toasts.len(), toasts + 2, "each try says why");
    assert_eq!(
        app.toast.toasts.back().map(|t| t.message.as_str()),
        Some("At most 32 bands")
    );
}

#[test]
fn removing_a_band_keeps_the_selection_on_a_sensible_row() {
    let mut app = parametric_app(&three_bands());

    // The selected band goes: the one that slid into its place takes over.
    let _ = app.handle_eq_modal(M::SelectBand(1));
    let _ = app.handle_eq_modal(M::RemoveBand(1));
    assert_eq!(
        bands(&app),
        [three_bands()[0], three_bands()[2]],
        "the middle band is gone"
    );
    assert_eq!(app.eq_modal.selected, Some(1));

    // The selected last band goes: the new last one.
    let _ = app.handle_eq_modal(M::RemoveBand(1));
    assert_eq!(app.eq_modal.selected, Some(0));

    // A band above the selection goes: the selection moves up with its band.
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::SelectBand(2));
    let _ = app.handle_eq_modal(M::RemoveBand(0));
    assert_eq!(app.eq_modal.selected, Some(1));
    assert_eq!(bands(&app)[1], three_bands()[2]);

    // The last band goes: nothing is left to select.
    let mut app = parametric_app(&[peak(500.0, 1.0, 1.0)]);
    let _ = app.handle_eq_modal(M::SelectBand(0));
    let _ = app.handle_eq_modal(M::RemoveBand(0));
    assert!(bands(&app).is_empty());
    assert_eq!(app.eq_modal.selected, None);
}

// ----------------------------------------------------------------------------
// Curve and row edits
// ----------------------------------------------------------------------------

#[test]
fn a_curve_point_press_selects_its_band_and_commits_a_typed_field() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Gain, "-".into()));
    let _ = app.handle_eq_modal(M::PointPressed(2));
    assert_eq!(app.eq_modal.selected, Some(2));
    assert_eq!(app.eq_modal.edit, None, "the field lost focus to the press");
    let _ = app.handle_eq_modal(M::SelectBand(1));
    assert_eq!(app.eq_modal.selected, Some(1));
}

#[test]
fn a_drag_moves_the_band_live_and_persists_at_its_end() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::BandDragged(1, 1_200.0, 5.5));
    assert_eq!(bands(&app)[1], peak(1_200.0, 5.5, 1.41));
    assert_eq!(app.eq_modal.selected, Some(1));
    assert!(app.eq_modal.bands_unsaved, "a move waits for the release");

    let _ = app.handle_eq_modal(M::BandDragEnded);
    assert!(!app.eq_modal.bands_unsaved, "the release persists the drag");
}

#[test]
fn a_dragged_pass_filter_keeps_its_gain() {
    let cut = EqBand::new(EqBandKind::HighPass, 30.0, 2.0, EQ_DEFAULT_Q);
    let mut app = parametric_app(&[cut]);
    let _ = app.handle_eq_modal(M::BandDragged(0, 45.0, 0.0));
    assert_eq!(bands(&app)[0].freq, 45.0);
    assert_eq!(bands(&app)[0].gain, 2.0, "pass filters ignore gain edits");
}

#[test]
fn the_wheel_q_the_type_and_the_switch_edit_one_band() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::BandQStepped(0, 2.24));
    assert_eq!(bands(&app)[0].q, 2.24);

    let _ = app.handle_eq_modal(M::BandKindSelected(1, EqBandKind::LowShelf));
    assert_eq!(bands(&app)[1].kind, EqBandKind::LowShelf);
    assert_eq!(bands(&app)[1].gain, -2.0, "the gain carries over");
    assert_eq!(app.eq_modal.selected, Some(1));

    let _ = app.handle_eq_modal(M::ToggleBand(2));
    assert!(!bands(&app)[2].enabled);
    let _ = app.handle_eq_modal(M::ToggleBand(2));
    assert!(bands(&app)[2].enabled);
    assert!(!app.eq_modal.bands_unsaved, "each of them persists at once");
}

#[test]
fn reset_zeroes_every_gain_and_keeps_the_bands() {
    let cut = EqBand::new(EqBandKind::LowPass, 15_000.0, 3.0, 0.9);
    let mut list = three_bands().to_vec();
    list.push(cut);
    let mut app = parametric_app(&list);
    let gains = [2.0; 10];
    app.playback.eq_state.set_all_gains(&gains);

    let _ = app.handle_eq_modal(M::ResetAll);

    let after = bands(&app);
    assert_eq!(after.len(), 4, "the bands stay");
    for (before, now) in list.iter().zip(&after) {
        assert_eq!(
            (now.kind, now.freq, now.q),
            (before.kind, before.freq, before.q)
        );
    }
    assert!(after[..3].iter().all(|band| band.gain == 0.0));
    assert_eq!(after[3], cut, "a pass filter stays as it is");
    assert_eq!(
        app.playback.eq_state.graphic_gains(),
        gains,
        "the Graphic sliders are another layout's"
    );
}

#[test]
fn copy_from_graphic_starts_from_the_sliders() {
    let mut app = parametric_app(&[]);
    let gains = [1.0, 2.0, 3.0, 4.0, 5.0, -1.0, -2.0, -3.0, -4.0, -5.0];
    app.playback.eq_state.set_all_gains(&gains);
    let _ = app.handle_eq_modal(M::CopyFromGraphic);
    assert_eq!(bands(&app), graphic_bands(&gains));
    assert_eq!(app.playback.eq_state.mode(), EqMode::Parametric);
}

// ----------------------------------------------------------------------------
// Typed fields
// ----------------------------------------------------------------------------

#[test]
fn typed_text_plays_as_soon_as_it_parses_and_stays_as_typed() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(1, BandField::Freq, "2.5k".into()));
    assert_eq!(bands(&app)[1].freq, 2_500.0);
    let edit = app
        .eq_modal
        .edit
        .as_ref()
        .expect("the field is being edited");
    assert_eq!(
        (edit.band, edit.field, edit.text.as_str()),
        (1, BandField::Freq, "2.5k")
    );
    assert_eq!(app.eq_modal.selected, Some(1), "typing selects the band");

    // Clamped in the audio, as typed in the field.
    let _ = app.handle_eq_modal(M::FieldInput(1, BandField::Freq, "50000".into()));
    assert_eq!(bands(&app)[1].freq, EQ_FREQ_MAX);
    assert_eq!(
        app.eq_modal.edit.as_ref().map(|e| e.text.as_str()),
        Some("50000")
    );
}

#[test]
fn text_that_does_not_parse_changes_nothing() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Gain, "-".into()));
    assert_eq!(bands(&app), three_bands());
    assert_eq!(
        app.eq_modal.edit.as_ref().map(|e| e.text.as_str()),
        Some("-"),
        "the field keeps what was typed"
    );
    assert!(!app.eq_modal.bands_unsaved);

    // Enter drops it: the field shows the stored value again.
    let _ = app.handle_eq_modal(M::FieldSubmit(0, BandField::Gain));
    assert_eq!(app.eq_modal.edit, None);
    assert_eq!(bands(&app), three_bands());
}

#[test]
fn enter_commits_the_value_and_normalizes_the_field() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Q, "2.50".into()));
    assert!(app.eq_modal.bands_unsaved);
    let _ = app.handle_eq_modal(M::FieldSubmit(0, BandField::Q));
    assert_eq!(bands(&app)[0].q, 2.5);
    assert_eq!(app.eq_modal.edit, None, "the field shows `2.5` again");
    assert!(!app.eq_modal.bands_unsaved, "Enter persists");
}

#[test]
fn typing_into_another_field_commits_the_first() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Gain, "6".into()));
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Gain, "6x".into()));
    // Unparsable at the switch: dropped, and the last value that parsed stays.
    let _ = app.handle_eq_modal(M::FieldInput(2, BandField::Q, "1".into()));
    assert_eq!(bands(&app)[0].gain, 6.0);
    assert_eq!(bands(&app)[2].q, 1.0);
    let edit = app.eq_modal.edit.as_ref().expect("editing the new field");
    assert_eq!((edit.band, edit.field), (2, BandField::Q));
}

#[test]
fn a_pass_filters_gain_field_takes_nothing() {
    let cut = EqBand::new(EqBandKind::HighPass, 30.0, 0.0, EQ_DEFAULT_Q);
    let mut app = parametric_app(&[cut]);
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Gain, "-6".into()));
    assert_eq!(bands(&app), [cut]);
    assert_eq!(app.eq_modal.edit, None);
}

#[test]
fn tab_commits_the_field_and_selects_the_next_fields_band() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Q, "3".into()));
    let _ = app.handle_eq_modal(M::FieldKey(Some(field_id(0, BandField::Q)), FieldKey::Next));
    assert_eq!(app.eq_modal.edit, None, "Tab commits like Enter");
    assert_eq!(bands(&app)[0].q, 3.0);
    assert_eq!(app.eq_modal.selected, Some(1), "the next field is band 2's");

    // Shift+Tab from the first field wraps to the last band.
    let _ = app.handle_eq_modal(M::FieldKey(
        Some(field_id(0, BandField::Freq)),
        FieldKey::Previous,
    ));
    assert_eq!(app.eq_modal.selected, Some(2));
}

#[test]
fn up_and_down_nudge_the_focused_field() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldKey(
        Some(field_id(1, BandField::Freq)),
        FieldKey::Up,
    ));
    assert_eq!(bands(&app)[1].freq, 1_060.0);
    let _ = app.handle_eq_modal(M::FieldKey(
        Some(field_id(0, BandField::Gain)),
        FieldKey::Down,
    ));
    assert_eq!(bands(&app)[0].gain, 2.5);
    assert_eq!(app.eq_modal.selected, Some(0));
    assert!(
        app.eq_modal.bands_unsaved,
        "a held key doesn't write per step"
    );

    // Nothing focused: the arrows do nothing.
    let before = bands(&app);
    let _ = app.handle_eq_modal(M::FieldKey(None, FieldKey::Up));
    let _ = app.handle_eq_modal(M::FieldKey(
        Some(iced::widget::Id::new("some-search")),
        FieldKey::Down,
    ));
    assert_eq!(bands(&app), before);

    // Close writes what the nudges left unsaved.
    let _ = app.handle_eq_modal(M::Close);
    assert!(!app.eq_modal.bands_unsaved);
}

#[test]
fn the_table_takes_tab_and_the_arrows_only_in_the_parametric_view() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut app = parametric_app(&three_bands());
    let tab = Key::Named(Named::Tab);
    let up = Key::Named(Named::ArrowUp);
    assert_eq!(
        app.eq_table_key(&tab, Modifiers::empty()),
        Some(FieldKey::Next)
    );
    assert_eq!(
        app.eq_table_key(&tab, Modifiers::SHIFT),
        Some(FieldKey::Previous)
    );
    assert_eq!(
        app.eq_table_key(&up, Modifiers::empty()),
        Some(FieldKey::Up)
    );
    assert_eq!(app.eq_table_key(&up, Modifiers::SHIFT), None);
    assert_eq!(
        app.eq_table_key(&Key::Character("x".into()), Modifiers::empty()),
        None
    );

    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Graphic));
    assert_eq!(app.eq_table_key(&tab, Modifiers::empty()), None);

    // Under another modal EQ takes no key.
    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Parametric));
    app.text_input_dialog.visible = true;
    assert_eq!(app.eq_table_key(&tab, Modifiers::empty()), None);
}

#[test]
fn escape_still_closes_the_modal_while_a_field_is_edited() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Freq, "12".into()));
    let _ = app.update(crate::Message::RawKeyEvent(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        None,
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Captured,
        false,
    ));
    assert!(!app.eq_modal.open);
    assert_eq!(app.eq_modal.edit, None, "the typed value is committed");
    assert_eq!(bands(&app)[0].freq, 12.0);
}

#[test]
fn a_hotkey_typed_into_a_field_does_not_fire() {
    let mut app = parametric_app(&three_bands());
    assert!(!app.modes.random);
    let _ = app.update(crate::Message::RawKeyEvent(
        iced::keyboard::Key::Character("x".into()),
        None,
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Captured,
        false,
    ));
    assert!(!app.modes.random);
    assert!(app.eq_modal.open);
}

// ----------------------------------------------------------------------------
// The list changing under the editor
// ----------------------------------------------------------------------------

#[test]
fn indices_past_a_shrunken_list_change_nothing() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::SelectBand(2));
    let _ = app.handle_eq_modal(M::FieldInput(2, BandField::Gain, "5".into()));
    // A config.toml reload replaces the list under the editor.
    let reloaded = [peak(60.0, 1.0, 1.0)];
    app.playback.eq_state.set_parametric_bands(&reloaded);

    for msg in [
        M::FieldInput(2, BandField::Gain, "7".into()),
        M::FieldSubmit(2, BandField::Gain),
        M::BandDragged(2, 300.0, 1.0),
        M::BandQStepped(2, 3.0),
        M::BandKindSelected(2, EqBandKind::LowShelf),
        M::ToggleBand(2),
        M::RemoveBand(2),
        M::SelectBand(5),
        M::PointPressed(4),
        M::FieldKey(Some(field_id(2, BandField::Q)), FieldKey::Up),
    ] {
        let _ = app.handle_eq_modal(msg);
    }
    assert_eq!(bands(&app), reloaded, "no band past the end is touched");
    assert_eq!(app.eq_modal.edit, None, "the stale edit is gone");
    assert!(app.eq_modal.selected.is_none_or(|i| i < 1));
}

// ----------------------------------------------------------------------------
// Import
// ----------------------------------------------------------------------------

#[test]
fn an_import_replaces_the_list_switches_to_parametric_and_turns_the_eq_on() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Graphic));
    let _ = app.handle_eq_modal(M::SelectBand(1));
    let _ = app.handle_eq_modal(M::FieldInput(1, BandField::Q, "9".into()));
    let imported = vec![
        EqBand::new(EqBandKind::LowShelf, 105.0, 6.5, 0.7),
        peak(220.0, -3.1, 0.85),
    ];

    let _ = app.handle_eq_modal(M::ImportLoaded(Ok((
        "HD 650 ParametricEQ.txt".into(),
        imported.clone(),
    ))));

    assert_eq!(bands(&app), imported);
    assert_eq!(app.playback.eq_state.mode(), EqMode::Parametric);
    assert!(app.playback.eq_state.is_enabled());
    assert_eq!(app.eq_modal.selected, None);
    assert_eq!(app.eq_modal.edit, None);
}

#[test]
fn a_failed_import_changes_nothing_and_says_why() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::SelectBand(1));
    let toasts = app.toast.toasts.len();

    let _ = app.handle_eq_modal(M::ImportLoaded(Err(
        "Couldn't import bad.txt: line 4: unsupported filter type BP".into(),
    )));

    assert_eq!(bands(&app), three_bands());
    assert_eq!(app.playback.eq_state.mode(), EqMode::Parametric);
    assert!(!app.playback.eq_state.is_enabled());
    assert_eq!(app.eq_modal.selected, Some(1));
    assert_eq!(app.toast.toasts.len(), toasts + 1);
    let toast = app.toast.toasts.back().expect("an error toast");
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Error);
    assert!(toast.message.contains("bad.txt"), "{}", toast.message);
}

#[test]
fn an_empty_list_is_not_exported() {
    let mut app = parametric_app(&[]);
    let toasts = app.toast.toasts.len();
    let _ = app.handle_eq_modal(M::Export);
    assert_eq!(app.toast.toasts.len(), toasts + 1);
    assert_eq!(
        app.toast.toasts.back().map(|t| t.level),
        Some(nokkvi_data::types::toast::ToastLevel::Warning)
    );
}

// ----------------------------------------------------------------------------
// Close and logout
// ----------------------------------------------------------------------------

#[test]
fn close_commits_a_pending_field_and_flushes_a_drag() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Freq, "90".into()));
    let _ = app.handle_eq_modal(M::Close);
    assert!(!app.eq_modal.open);
    assert_eq!(app.eq_modal.edit, None);
    assert!(!app.eq_modal.bands_unsaved);
    assert_eq!(bands(&app)[0].freq, 90.0);
}

#[test]
fn logout_clears_the_editor_state() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::SelectBand(1));
    let _ = app.handle_eq_modal(M::FieldInput(1, BandField::Gain, "-".into()));
    let _ = app.handle_eq_modal(M::BandDragged(0, 150.0, 1.0));

    app.discard_modal(ActiveModal::Eq);

    assert!(!app.eq_modal.open);
    assert_eq!(app.eq_modal.selected, None);
    assert_eq!(app.eq_modal.edit, None);
    assert!(!app.eq_modal.bands_unsaved);
}

// ----------------------------------------------------------------------------
// Graphic sliders: a drag plays every step and writes once
// ----------------------------------------------------------------------------

/// An app with the EQ modal open on the Graphic view.
fn graphic_app() -> Nokkvi {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = crate::View::Queue;
    let _ = app.handle_eq_modal(M::Open);
    app
}

/// Every drag step used to write config.toml and the database, reload the
/// queue and push an empty toast (nine of each for one short drag).
#[test]
fn a_slider_drag_plays_each_step_and_writes_at_its_end() {
    let mut app = graphic_app();
    let _ = app.handle_eq_modal(M::GainChanged(2, 1.5));
    let _ = app.handle_eq_modal(M::GainChanged(2, 4.0));
    assert_eq!(app.playback.eq_state.get_band_gain(2), 4.0);
    assert!(app.eq_modal.gains_unsaved, "a step waits for the release");
    assert!(app.toast.toasts.is_empty(), "a step says nothing");

    let _ = app.handle_eq_modal(M::GainCommitted);
    assert!(!app.eq_modal.gains_unsaved, "the release writes the gains");
    assert!(app.toast.toasts.is_empty());
}

#[test]
fn a_commit_with_nothing_moved_writes_nothing() {
    let mut app = graphic_app();
    let _ = app.handle_eq_modal(M::GainCommitted);
    assert!(!app.eq_modal.gains_unsaved);
}

/// A release the window never saw (the pointer let go over another app)
/// leaves the last step unsaved: closing the modal writes it.
#[test]
fn close_writes_a_slider_drag_whose_release_was_missed() {
    let mut app = graphic_app();
    let _ = app.handle_eq_modal(M::GainChanged(0, -3.0));
    let _ = app.handle_eq_modal(M::Close);
    assert!(!app.eq_modal.open);
    assert!(!app.eq_modal.gains_unsaved);

    let mut app = graphic_app();
    let _ = app.handle_eq_modal(M::GainChanged(0, -3.0));
    let _ = app.handle_eq_modal(M::Toggle);
    assert!(!app.eq_modal.open);
    assert!(!app.eq_modal.gains_unsaved);
}

/// A preset and Reset write all ten gains themselves, so they also settle a
/// drag left unsaved.
#[test]
fn a_preset_and_reset_leave_no_slider_step_unsaved() {
    let mut app = graphic_app();
    let _ = app.handle_eq_modal(M::GainChanged(5, 2.0));
    let _ = app.handle_eq_modal(M::PresetSelected(crate::widgets::PresetChoice::Builtin(0)));
    assert!(!app.eq_modal.gains_unsaved);

    let _ = app.handle_eq_modal(M::GainChanged(5, 2.0));
    let _ = app.handle_eq_modal(M::ResetAll);
    assert!(!app.eq_modal.gains_unsaved);
    assert_eq!(app.playback.eq_state.get_band_gain(5), 0.0);
}

#[test]
fn a_slider_past_the_tenth_band_changes_nothing() {
    let mut app = graphic_app();
    let _ = app.handle_eq_modal(M::GainChanged(10, 6.0));
    assert!(!app.eq_modal.gains_unsaved);
}

// ----------------------------------------------------------------------------
// Saved profiles
// ----------------------------------------------------------------------------

fn profile(name: &str, bands: &[EqBand]) -> EqProfile {
    EqProfile {
        name: name.to_owned(),
        bands: bands.to_vec(),
    }
}

fn speakers() -> Vec<EqBand> {
    vec![
        EqBand::new(EqBandKind::HighPass, 45.0, 0.0, 0.5),
        peak(120.0, -4.0, 2.0),
    ]
}

fn last_toast(app: &Nokkvi) -> Option<(nokkvi_data::types::toast::ToastLevel, String)> {
    app.toast
        .toasts
        .back()
        .map(|toast| (toast.level, toast.message.clone()))
}

#[test]
fn selecting_a_profile_plays_its_bands_and_turns_the_eq_on() {
    let mut app = parametric_app(&three_bands());
    app.eq_modal.profiles = vec![
        profile("Headphones", &three_bands()),
        profile("Speakers", &speakers()),
    ];
    let _ = app.handle_eq_modal(M::SelectBand(1));
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Gain, "-".into()));
    assert!(!app.playback.eq_state.is_enabled());

    let _ = app.handle_eq_modal(M::ProfileSelected(1));

    assert_eq!(bands(&app), speakers());
    assert!(app.playback.eq_state.is_enabled());
    assert_eq!(app.playback.eq_state.mode(), EqMode::Parametric);
    assert_eq!(app.eq_modal.selected, None);
    assert_eq!(app.eq_modal.edit, None);
    assert!(!app.eq_modal.bands_unsaved, "persisted at once");
    assert_eq!(app.eq_modal.matching_profile(&bands(&app)), Some(1));

    // A stale index changes nothing.
    let _ = app.handle_eq_modal(M::ProfileSelected(9));
    assert_eq!(bands(&app), speakers());
}

#[test]
fn a_profile_save_refuses_an_empty_or_taken_name() {
    let mut app = parametric_app(&three_bands());
    app.eq_modal.profiles = vec![profile("Speakers", &speakers())];

    let _ = app.handle_eq_modal(M::SavePreset);
    assert!(app.eq_modal.save_mode);
    assert!(app.eq_modal.save_name.is_empty());

    let _ = app.handle_eq_modal(M::SavePresetNameChanged("   ".into()));
    let _ = app.handle_eq_modal(M::SavePresetConfirm);
    assert_eq!(
        last_toast(&app),
        Some((
            nokkvi_data::types::toast::ToastLevel::Warning,
            "Profile name cannot be empty".to_owned()
        ))
    );
    assert!(app.eq_modal.save_mode, "the prompt stays for another try");

    let _ = app.handle_eq_modal(M::SavePresetNameChanged("SPEAKERS".into()));
    let _ = app.handle_eq_modal(M::SavePresetConfirm);
    assert_eq!(
        last_toast(&app),
        Some((
            nokkvi_data::types::toast::ToastLevel::Warning,
            "A profile named 'SPEAKERS' already exists".to_owned()
        ))
    );
    assert_eq!(app.eq_modal.profiles.len(), 1);
    assert!(app.eq_modal.save_mode);

    // A built-in Graphic preset's name is free for a profile.
    let _ = app.handle_eq_modal(M::SavePresetNameChanged("Rock".into()));
    let _ = app.handle_eq_modal(M::SavePresetConfirm);
    assert_eq!(app.eq_modal.profiles.len(), 2);
}

#[test]
fn an_empty_band_list_is_not_saved_as_a_profile() {
    let mut app = parametric_app(&[]);
    let toasts = app.toast.toasts.len();
    let _ = app.handle_eq_modal(M::SavePreset);
    assert!(!app.eq_modal.save_mode, "no prompt for nothing to save");
    assert_eq!(app.toast.toasts.len(), toasts + 1);
    assert_eq!(
        last_toast(&app),
        Some((
            nokkvi_data::types::toast::ToastLevel::Warning,
            "No bands to save".to_owned()
        ))
    );

    // The list emptied under an open prompt: refused at OK too.
    let mut app = parametric_app(&speakers());
    let _ = app.handle_eq_modal(M::SavePreset);
    app.playback.eq_state.set_parametric_bands(&[]);
    let _ = app.handle_eq_modal(M::SavePresetNameChanged("Nothing".into()));
    let _ = app.handle_eq_modal(M::SavePresetConfirm);
    assert!(app.eq_modal.profiles.is_empty());
}

#[test]
fn a_saved_profile_joins_the_list_and_shows_as_selected() {
    let mut app = parametric_app(&speakers());
    let _ = app.handle_eq_modal(M::FieldInput(1, BandField::Gain, "-5".into()));
    let _ = app.handle_eq_modal(M::SavePreset);
    assert_eq!(app.eq_modal.edit, None, "the Save press commits the field");
    let _ = app.handle_eq_modal(M::SavePresetNameChanged("  Desk speakers ".into()));
    let _ = app.handle_eq_modal(M::SavePresetConfirm);

    assert!(!app.eq_modal.save_mode);
    assert!(app.eq_modal.save_name.is_empty());
    assert_eq!(app.eq_modal.profiles.len(), 1);
    let saved = &app.eq_modal.profiles[0];
    assert_eq!(saved.name, "Desk speakers", "trimmed");
    assert_eq!(saved.bands[1].gain, -5.0, "the list as it plays");
    assert_eq!(app.eq_modal.matching_profile(&bands(&app)), Some(0));

    // An edit leaves the profile: the picker shows `Custom` again.
    let _ = app.handle_eq_modal(M::BandDragged(1, 130.0, -5.0));
    assert_eq!(app.eq_modal.matching_profile(&bands(&app)), None);
    // The Graphic layout's presets are untouched.
    assert!(app.eq_modal.custom_presets.is_empty());
}

#[test]
fn deleting_a_profile_keeps_the_band_list() {
    let mut app = parametric_app(&speakers());
    app.eq_modal.profiles = vec![
        profile("Headphones", &three_bands()),
        profile("Speakers", &speakers()),
    ];
    let _ = app.handle_eq_modal(M::ToggleEnabled);
    assert_eq!(app.eq_modal.matching_profile(&bands(&app)), Some(1));

    let _ = app.handle_eq_modal(M::DeleteProfile(1));

    assert_eq!(
        app.eq_modal.profiles,
        [profile("Headphones", &three_bands())]
    );
    assert_eq!(bands(&app), speakers(), "the bands play on");
    assert!(app.playback.eq_state.is_enabled());

    let _ = app.handle_eq_modal(M::DeleteProfile(5));
    assert_eq!(
        app.eq_modal.profiles.len(),
        1,
        "a stale index deletes nothing"
    );
}

/// A loaded profile used to stay only as the band list: switching to another
/// one lost it unless it was saved by hand first.
#[test]
fn an_import_is_saved_as_a_profile_under_its_files_name() {
    let imported = vec![
        EqBand::new(EqBandKind::LowShelf, 105.0, 6.5, 0.7),
        peak(220.0, -3.1, 0.85),
    ];
    let mut app = parametric_app(&three_bands());
    app.eq_modal.profiles = vec![profile("Speakers", &speakers())];

    let _ = app.handle_eq_modal(M::ImportLoaded(Ok((
        "HD 650 ParametricEQ.txt".into(),
        imported.clone(),
    ))));

    assert_eq!(
        app.eq_modal.profiles,
        [
            profile("Speakers", &speakers()),
            profile("HD 650", &imported)
        ]
    );
    assert_eq!(
        app.eq_modal.matching_profile(&bands(&app)),
        Some(1),
        "the picker shows it"
    );
    // Nothing is left to name: the Save prompt starts empty.
    let _ = app.handle_eq_modal(M::SavePreset);
    assert!(app.eq_modal.save_name.is_empty());
}

#[test]
fn importing_a_file_again_replaces_its_profile_where_it_sits() {
    let newer = vec![peak(1_000.0, -2.0, 1.0)];
    let mut app = parametric_app(&three_bands());
    app.eq_modal.profiles = vec![
        profile("Headphones", &three_bands()),
        profile("hd 650", &speakers()),
        profile("Speakers", &speakers()),
    ];

    let _ = app.handle_eq_modal(M::ImportLoaded(Ok(("HD 650.txt".into(), newer.clone()))));

    assert_eq!(
        app.eq_modal.profiles,
        [
            profile("Headphones", &three_bands()),
            profile("HD 650", &newer),
            profile("Speakers", &speakers()),
        ]
    );
    assert_eq!(app.eq_modal.matching_profile(&bands(&app)), Some(1));
}

#[test]
fn a_refused_import_saves_no_profile() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::ImportLoaded(Err("Couldn't import x.txt: why".into())));
    assert!(app.eq_modal.profiles.is_empty());
    assert_eq!(bands(&app), three_bands());
}

#[test]
fn settings_load_restores_the_profiles_and_logout_clears_them() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = parametric_app(&three_bands());
    let saved = vec![
        profile("Headphones", &three_bands()),
        profile("Speakers", &speakers()),
    ];
    let _ = app.handle_player_settings_loaded(
        nokkvi_data::types::player_settings::LivePlayerSettings {
            eq_mode: EqMode::Parametric,
            eq_bands: three_bands().to_vec(),
            eq_profiles: saved.clone(),
            ..nokkvi_data::types::player_settings::LivePlayerSettings::default()
        },
    );
    assert_eq!(app.eq_modal.profiles, saved);
    assert_eq!(app.eq_modal.matching_profile(&bands(&app)), Some(0));

    app.discard_modal(ActiveModal::Eq);
    assert!(app.eq_modal.profiles.is_empty());
}

// ----------------------------------------------------------------------------
// The AutoEq panel: search the AutoEq list, load a headphone's profile.
// Driven with canned results; nothing reaches the network (`test_app()` has
// no `AppService`, so the requests are dropped).
// ----------------------------------------------------------------------------

mod autoeq {
    use std::sync::Arc;

    use iced::{
        event::Status,
        keyboard::{Key, Modifiers, key::Named},
    };
    use nokkvi_data::{
        audio::eq::{EqBand, EqBandKind, EqMode},
        services::autoeq::AutoEqEntry,
        types::toast::ToastLevel,
    };

    use super::{bands, last_toast, parametric_app, peak, three_bands};
    use crate::{
        Nokkvi,
        update::modals::ActiveModal,
        widgets::{
            EqModalMessage as M,
            eq_autoeq::{AutoEqList, AutoEqMessage as A, AutoEqStep},
            eq_parametric::{BandField, FieldKey},
        },
    };

    fn entry(name: &str, source: &str, rig: Option<&str>) -> AutoEqEntry {
        AutoEqEntry {
            name: name.to_owned(),
            source: source.to_owned(),
            rig: rig.map(str::to_owned),
            path: format!("{source}/over-ear/{}", name.replace(' ', "%20")),
        }
    }

    /// In the index's order (by name, then AutoEq's order of sources).
    fn list() -> Arc<Vec<AutoEqEntry>> {
        Arc::new(vec![
            entry("AKG K371", "oratory1990", None),
            entry("Sennheiser HD 600", "oratory1990", None),
            entry("Sennheiser HD 600", "crinacle", Some("GRAS 43AG-7")),
            entry("Sennheiser HD 650", "oratory1990", None),
        ])
    }

    fn hd600_bands() -> Vec<EqBand> {
        vec![
            EqBand::new(EqBandKind::LowShelf, 105.0, 6.5, 0.7),
            peak(220.0, -3.1, 0.85),
        ]
    }

    fn generation(app: &Nokkvi) -> u64 {
        app.eq_modal.autoeq_generation.current()
    }

    fn panel(app: &Nokkvi) -> &crate::widgets::eq_autoeq::AutoEqPanel {
        app.eq_modal.autoeq.as_ref().expect("the panel is open")
    }

    fn send(app: &mut Nokkvi, msg: A) {
        let _ = app.handle_eq_modal(M::AutoEq(msg));
    }

    /// The panel open over `three_bands()` with `list()` in it.
    fn panel_app() -> Nokkvi {
        let mut app = parametric_app(&three_bands());
        send(&mut app, A::Open);
        let generation = generation(&app);
        send(
            &mut app,
            A::IndexLoaded {
                generation,
                result: Ok(list()),
            },
        );
        app
    }

    fn shown_names(app: &Nokkvi) -> Vec<String> {
        let panel = panel(app);
        let AutoEqList::Ready(list) = &panel.list else {
            panic!("the list is not ready");
        };
        panel
            .matches
            .shown
            .iter()
            .map(|&i| format!("{} / {}", list[i].name, list[i].by_line()))
            .collect()
    }

    fn raw_key(app: &mut Nokkvi, key: Key, status: Status) {
        let _ = app.update(crate::Message::RawKeyEvent(
            key,
            None,
            Modifiers::empty(),
            status,
            false,
        ));
    }

    #[test]
    fn opening_the_panel_asks_for_the_list_and_bumps_the_generation() {
        let mut app = parametric_app(&three_bands());
        let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Gain, "5".into()));
        let before = generation(&app);

        send(&mut app, A::Open);

        let panel = panel(&app);
        assert!(matches!(panel.list, AutoEqList::Loading { refresh: false }));
        assert!(panel.query.is_empty());
        assert_eq!(generation(&app), before.wrapping_add(1));
        assert_eq!(app.eq_modal.edit, None, "the press commits a typed field");
        assert_eq!(bands(&app)[0].gain, 5.0, "the bands are left alone");
    }

    #[test]
    fn the_panel_opens_only_in_the_parametric_view() {
        let mut app = parametric_app(&three_bands());
        let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Graphic));
        let before = generation(&app);
        send(&mut app, A::Open);
        assert!(app.eq_modal.autoeq.is_none());
        assert_eq!(generation(&app), before, "nothing was asked");
    }

    #[test]
    fn a_list_result_fills_the_panel_with_the_best_match_highlighted() {
        let app = panel_app();
        let panel = panel(&app);
        assert!(matches!(&panel.list, AutoEqList::Ready(list) if list.len() == 4));
        assert_eq!(panel.matches.matched, 4);
        assert_eq!(panel.highlight, Some(0));
        assert_eq!(panel.downloading, None);
    }

    #[test]
    fn a_stale_list_result_is_dropped() {
        let mut app = parametric_app(&three_bands());
        send(&mut app, A::Open);
        let first = generation(&app);
        // Back and open again: the first open's answer is not this panel's.
        send(&mut app, A::Back);
        send(&mut app, A::Open);
        send(
            &mut app,
            A::IndexLoaded {
                generation: first,
                result: Ok(list()),
            },
        );
        assert!(matches!(panel(&app).list, AutoEqList::Loading { .. }));

        // A result for a closed panel opens nothing.
        let current = generation(&app);
        send(&mut app, A::Back);
        send(
            &mut app,
            A::IndexLoaded {
                generation: current,
                result: Ok(list()),
            },
        );
        assert!(app.eq_modal.autoeq.is_none());
    }

    #[test]
    fn typing_filters_the_list_at_once() {
        let mut app = panel_app();
        send(&mut app, A::SearchChanged("sennheiser HD 600".into()));
        assert_eq!(
            shown_names(&app),
            [
                "Sennheiser HD 600 / oratory1990",
                "Sennheiser HD 600 / crinacle on GRAS 43AG-7",
            ],
            "every term, any case; equal names keep the list's order"
        );
        send(&mut app, A::SearchChanged("sennheiser".into()));
        assert_eq!(panel(&app).matches.matched, 3);
        assert_eq!(panel(&app).highlight, Some(0));

        send(&mut app, A::SearchChanged("crinacle".into()));
        assert_eq!(
            shown_names(&app),
            ["Sennheiser HD 600 / crinacle on GRAS 43AG-7"],
            "a term may match the source"
        );

        send(&mut app, A::SearchChanged("zzz".into()));
        assert!(panel(&app).matches.shown.is_empty(), "No match");
        assert_eq!(panel(&app).highlight, None);
        // Enter on no match loads nothing.
        let before = generation(&app);
        send(&mut app, A::Submit);
        assert_eq!(generation(&app), before);
    }

    #[test]
    fn up_and_down_move_the_highlight_and_enter_loads_it() {
        let mut app = panel_app();
        send(&mut app, A::Step(AutoEqStep::Down));
        send(&mut app, A::Step(AutoEqStep::Down));
        send(&mut app, A::Step(AutoEqStep::Up));
        assert_eq!(panel(&app).highlight, Some(1));

        let before = generation(&app);
        send(&mut app, A::Submit);
        assert_eq!(panel(&app).downloading, Some(1));
        assert_eq!(generation(&app), before.wrapping_add(1));
        // The same row again asks nothing more.
        send(&mut app, A::Choose(1));
        assert_eq!(generation(&app), before.wrapping_add(1));
    }

    #[test]
    fn the_arrows_reach_the_panel_from_the_search_field() {
        let mut app = panel_app();
        // A focused search field captures the arrows.
        raw_key(&mut app, Key::Named(Named::ArrowDown), Status::Captured);
        assert_eq!(panel(&app).highlight, Some(1));
        raw_key(&mut app, Key::Named(Named::ArrowUp), Status::Captured);
        assert_eq!(panel(&app).highlight, Some(0));
        // A letter typed there stays the field's.
        raw_key(&mut app, Key::Character("x".into()), Status::Captured);
        assert!(!app.modes.random);
        assert!(app.eq_modal.autoeq.is_some());
    }

    #[test]
    fn the_panel_takes_its_keys_and_the_band_table_stands_down() {
        let mut app = panel_app();
        let tab = Key::Named(Named::Tab);
        let down = Key::Named(Named::ArrowDown);
        let enter = Key::Named(Named::Enter);
        assert_eq!(app.eq_table_key(&tab, Modifiers::empty()), None);
        assert_eq!(app.eq_table_key(&down, Modifiers::empty()), None);
        assert!(matches!(
            app.eq_autoeq_key(&down, Modifiers::empty(), Status::Captured),
            Some(A::Step(AutoEqStep::Down))
        ));
        assert!(
            app.eq_autoeq_key(&down, Modifiers::SHIFT, Status::Captured)
                .is_none()
        );
        // A focused field's Enter is its own submit; an Enter nothing took
        // loads the highlighted row.
        assert!(
            app.eq_autoeq_key(&enter, Modifiers::empty(), Status::Captured)
                .is_none()
        );
        assert!(matches!(
            app.eq_autoeq_key(&enter, Modifiers::empty(), Status::Ignored),
            Some(A::Submit)
        ));

        // Under another modal the panel takes nothing.
        app.text_input_dialog.visible = true;
        assert!(
            app.eq_autoeq_key(&down, Modifiers::empty(), Status::Captured)
                .is_none()
        );
        app.text_input_dialog.visible = false;

        // Back in the editor, the table has its keys again.
        send(&mut app, A::Back);
        assert!(
            app.eq_autoeq_key(&down, Modifiers::empty(), Status::Captured)
                .is_none()
        );
        assert_eq!(
            app.eq_table_key(&tab, Modifiers::empty()),
            Some(FieldKey::Next)
        );
    }

    #[test]
    fn a_chosen_profile_goes_in_like_an_import_and_returns_to_the_editor() {
        let mut app = panel_app();
        assert!(!app.playback.eq_state.is_enabled());
        app.eq_modal.selected = Some(1);
        let hd600 = list()[2].clone();
        send(&mut app, A::Choose(2));
        let generation = generation(&app);

        send(
            &mut app,
            A::ProfileLoaded {
                generation,
                entry: hd600,
                result: Ok(hd600_bands()),
            },
        );

        assert_eq!(bands(&app), hd600_bands());
        assert_eq!(app.playback.eq_state.mode(), EqMode::Parametric);
        assert!(app.playback.eq_state.is_enabled());
        assert!(app.eq_modal.autoeq.is_none(), "back to the editor");
        assert_eq!(app.eq_modal.selected, None);
        assert_eq!(app.eq_modal.edit, None);
        assert!(!app.eq_modal.bands_unsaved, "persisted at once");
        // It is in the profile list under the headphone's name, and shown.
        assert_eq!(
            app.eq_modal.profiles,
            [super::profile("Sennheiser HD 600", &hd600_bands())]
        );
        assert_eq!(app.eq_modal.matching_profile(&bands(&app)), Some(0));
    }

    #[test]
    fn a_failed_profile_keeps_the_panel_and_says_why() {
        let mut app = panel_app();
        send(&mut app, A::Choose(3));
        let generation = generation(&app);
        send(
            &mut app,
            A::ProfileLoaded {
                generation,
                entry: list()[3].clone(),
                result: Err("GitHub answered HTTP 404".into()),
            },
        );
        assert_eq!(bands(&app), three_bands(), "nothing changed");
        assert!(!app.playback.eq_state.is_enabled());
        let panel = panel(&app);
        assert_eq!(panel.downloading, None, "the row can be tried again");
        assert!(matches!(panel.list, AutoEqList::Ready(_)));
        assert_eq!(
            last_toast(&app),
            Some((
                ToastLevel::Error,
                "Couldn't load Sennheiser HD 650: GitHub answered HTTP 404".to_owned()
            ))
        );
    }

    #[test]
    fn a_stale_profile_result_is_dropped() {
        let mut app = panel_app();
        send(&mut app, A::Choose(1));
        let first = generation(&app);
        send(&mut app, A::Choose(3));
        let second = generation(&app);

        // The first press was superseded by the second.
        send(
            &mut app,
            A::ProfileLoaded {
                generation: first,
                entry: list()[1].clone(),
                result: Ok(hd600_bands()),
            },
        );
        assert_eq!(bands(&app), three_bands());
        assert_eq!(panel(&app).downloading, Some(3));

        // A panel closed meanwhile takes nothing, the current answer neither.
        send(&mut app, A::Back);
        send(
            &mut app,
            A::ProfileLoaded {
                generation: second,
                entry: list()[3].clone(),
                result: Ok(hd600_bands()),
            },
        );
        assert_eq!(bands(&app), three_bands());
        assert!(!app.playback.eq_state.is_enabled());
    }

    #[test]
    fn a_failed_list_offers_retry_the_way_it_was_asked() {
        let mut app = panel_app();
        send(&mut app, A::Refresh);
        assert!(matches!(
            panel(&app).list,
            AutoEqList::Loading { refresh: true }
        ));
        assert!(panel(&app).matches.shown.is_empty());
        let generation = generation(&app);
        send(
            &mut app,
            A::IndexLoaded {
                generation,
                result: Err("couldn't reach GitHub (error sending request)".into()),
            },
        );
        assert!(matches!(
            &panel(&app).list,
            AutoEqList::Failed { refresh: true, why } if why.contains("couldn't reach GitHub")
        ));
        let before = self::generation(&app);
        send(&mut app, A::Retry);
        assert!(matches!(
            panel(&app).list,
            AutoEqList::Loading { refresh: true }
        ));
        assert_eq!(self::generation(&app), before.wrapping_add(1));
        assert!(app.eq_modal.autoeq.is_some(), "the panel stays");
    }

    #[test]
    fn escape_backs_out_of_the_panel_then_closes_the_modal() {
        let mut app = panel_app();
        raw_key(&mut app, Key::Named(Named::Escape), Status::Captured);
        assert!(app.eq_modal.open, "one level a press");
        assert!(app.eq_modal.autoeq.is_none());
        raw_key(&mut app, Key::Named(Named::Escape), Status::Ignored);
        assert!(!app.eq_modal.open);
    }

    #[test]
    fn closing_the_modal_or_switching_to_graphic_drops_the_panel() {
        let mut app = panel_app();
        let _ = app.handle_eq_modal(M::Close);
        assert!(app.eq_modal.autoeq.is_none());
        let _ = app.handle_eq_modal(M::Open);
        assert!(app.eq_modal.autoeq.is_none(), "it reopens on the editor");

        send(&mut app, A::Open);
        let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Graphic));
        assert!(app.eq_modal.autoeq.is_none());
        let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Parametric));
        assert!(app.eq_modal.autoeq.is_none());

        send(&mut app, A::Open);
        let _ = app.handle_eq_modal(M::Toggle);
        assert!(!app.eq_modal.open);
        assert!(app.eq_modal.autoeq.is_none());
    }

    #[test]
    fn logout_clears_the_panel_and_carries_the_generation_forward() {
        let mut app = panel_app();
        send(&mut app, A::Choose(1));
        let in_flight = generation(&app);

        app.discard_modal(ActiveModal::Eq);

        assert!(app.eq_modal.autoeq.is_none());
        assert!(!app.eq_modal.open);
        assert!(
            !app.eq_modal.autoeq_generation.accepts(in_flight),
            "a pre-logout answer can't match the next session"
        );
        assert_eq!(generation(&app), in_flight.wrapping_add(1));
    }
}
