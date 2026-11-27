//! The equalizer modal's Parametric view: the view switch, the band list
//! edits, the typed fields (and their widget remounts), import, saved
//! profiles and the modal's teardown.
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
// Field remounts: a value set from outside a field gives it a fresh widget
// state (iced's text_input redraws an outside value in the fallback font
// while its state is kept; `FieldMounts`)
// ----------------------------------------------------------------------------

/// Every field's mount key, band by band (Freq, Gain, Q).
fn keys(app: &Nokkvi) -> Vec<[u64; 3]> {
    (0..bands(app).len())
        .map(|band| BandField::ALL.map(|field| app.eq_modal.mounts.key(band, field)))
        .collect()
}

fn key(app: &Nokkvi, band: usize, field: BandField) -> u64 {
    app.eq_modal.mounts.key(band, field)
}

/// The fields whose key moved between two snapshots, as (band, field).
fn rekeyed(before: &[[u64; 3]], after: &[[u64; 3]]) -> Vec<(usize, BandField)> {
    after
        .iter()
        .enumerate()
        .flat_map(|(band, fields)| {
            BandField::ALL
                .into_iter()
                .enumerate()
                .filter(move |&(i, _)| before.get(band).is_none_or(|old| old[i] != fields[i]))
                .map(move |(_, field)| (band, field))
        })
        .collect()
}

#[test]
fn a_freshly_opened_modal_starts_in_sync() {
    let app = parametric_app(&three_bands());
    let before = keys(&app);
    assert!(
        before.iter().flatten().all(|&k| k != 0),
        "every field was keyed at Open: {before:?}"
    );
    let mut app = app;
    app.sync_eq_fields();
    assert_eq!(keys(&app), before, "a second sync changes nothing");
}

#[test]
fn a_curve_drag_and_a_wheel_step_rekey_the_fields_they_change() {
    let mut app = parametric_app(&three_bands());
    let before = keys(&app);
    let _ = app.handle_eq_modal(M::BandDragged(1, 1_200.0, 5.5));
    let dragged = keys(&app);
    assert_eq!(
        rekeyed(&before, &dragged),
        [(1, BandField::Freq), (1, BandField::Gain)]
    );

    let _ = app.handle_eq_modal(M::BandQStepped(0, 2.24));
    assert_eq!(rekeyed(&dragged, &keys(&app)), [(0, BandField::Q)]);
}

#[test]
fn a_nudge_rekeys_the_focused_field_and_hands_its_focus_back() {
    let mut app = parametric_app(&three_bands());
    let before = keys(&app);
    let task = app.handle_eq_modal(M::FieldKey(
        Some(field_id(1, BandField::Freq)),
        FieldKey::Up,
    ));
    assert_eq!(rekeyed(&before, &keys(&app)), [(1, BandField::Freq)]);
    assert!(task.units() > 0, "the re-keyed field is focused again");

    // At a limit nothing changes: no re-key, no focus task.
    let top = peak(EQ_FREQ_MAX, 0.0, 1.0);
    let mut app = parametric_app(&[top]);
    let before = keys(&app);
    let task = app.handle_eq_modal(M::FieldKey(
        Some(field_id(0, BandField::Freq)),
        FieldKey::Up,
    ));
    assert_eq!(keys(&app), before);
    assert_eq!(task.units(), 0);
}

#[test]
fn typing_keeps_the_fields_key_and_committing_a_different_text_rekeys_it() {
    let mut app = parametric_app(&three_bands());
    let typed_into = key(&app, 0, BandField::Freq);

    // The first keystroke and every one after it: the widget holds the text.
    for typed in ["2", "25", "250", "2500"] {
        let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Freq, typed.into()));
        assert_eq!(key(&app, 0, BandField::Freq), typed_into, "{typed}");
    }
    assert_eq!(bands(&app)[0].freq, 2_500.0, "the value plays as typed");

    // Enter normalizes `2500` to `2.5k`: a new text from outside the field.
    let _ = app.handle_eq_modal(M::FieldSubmit(0, BandField::Freq));
    assert_ne!(key(&app, 0, BandField::Freq), typed_into);

    // A commit that shows exactly what was typed changes nothing.
    let _ = app.handle_eq_modal(M::FieldInput(1, BandField::Q, "1.41".into()));
    let q = key(&app, 1, BandField::Q);
    let _ = app.handle_eq_modal(M::FieldSubmit(1, BandField::Q));
    assert_eq!(key(&app, 1, BandField::Q), q);
}

#[test]
fn typing_into_another_field_rekeys_only_the_one_left_behind() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(0, BandField::Gain, "-4".into()));
    let before = keys(&app);
    // Field A (the first band's gain) still shows `-4`; typing into field B
    // commits it, so A now shows `-4.0`. B keeps its widget, focus and caret.
    let _ = app.handle_eq_modal(M::FieldInput(2, BandField::Q, "1".into()));
    assert_eq!(rekeyed(&before, &keys(&app)), [(0, BandField::Gain)]);
}

#[test]
fn reset_rekeys_the_gains_it_zeroes() {
    let mut list = three_bands().to_vec();
    list.push(peak(4_000.0, 0.0, 2.0));
    let mut app = parametric_app(&list);
    let before = keys(&app);
    let _ = app.handle_eq_modal(M::ResetAll);
    assert_eq!(
        rekeyed(&before, &keys(&app)),
        [
            (0, BandField::Gain),
            (1, BandField::Gain),
            (2, BandField::Gain)
        ],
        "the flat band's `0.0` stays as it was"
    );
}

#[test]
fn a_band_removed_above_rekeys_the_rows_that_slid_up() {
    let mut app = parametric_app(&three_bands());
    let before = keys(&app);
    let _ = app.handle_eq_modal(M::RemoveBand(0));
    let after = keys(&app);
    assert_eq!(after.len(), 2);
    // Every field of the two rows now shows the next band's values.
    assert_eq!(rekeyed(&before, &after).len(), 6, "{before:?} -> {after:?}");

    // A band added at the end shifts nothing.
    let _ = app.handle_eq_modal(M::AddBand);
    assert_eq!(&keys(&app)[..2], &after[..]);
}

#[test]
fn an_import_a_type_change_and_a_settings_reload_rekey_what_they_change() {
    let mut app = parametric_app(&three_bands());
    let before = keys(&app);
    let imported = vec![
        EqBand::new(EqBandKind::LowShelf, 105.0, 6.5, 0.7),
        peak(1_000.0, -2.0, 1.41),
    ];
    let _ = app.handle_eq_modal(M::ImportLoaded(Ok(("HD 650.txt".into(), imported))));
    // The second band is the same as before in every field.
    assert_eq!(
        rekeyed(&before, &keys(&app)),
        [
            (0, BandField::Freq),
            (0, BandField::Gain),
            (0, BandField::Q)
        ]
    );

    let before = keys(&app);
    let _ = app.handle_eq_modal(M::BandKindSelected(1, EqBandKind::LowPass));
    assert_eq!(
        rekeyed(&before, &keys(&app)),
        [(1, BandField::Gain)],
        "a pass filter's gain field goes blank"
    );

    // A config.toml reload replaces the list outside the modal's handler.
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let before = keys(&app);
    let _ = app.handle_player_settings_loaded(
        nokkvi_data::types::player_settings::LivePlayerSettings {
            eq_mode: EqMode::Parametric,
            eq_bands: vec![
                EqBand::new(EqBandKind::LowShelf, 105.0, 6.5, 0.7),
                peak(1_000.0, -2.0, 2.0),
            ],
            ..nokkvi_data::types::player_settings::LivePlayerSettings::default()
        },
    );
    assert_eq!(
        rekeyed(&before, &keys(&app)),
        [(1, BandField::Gain), (1, BandField::Q)]
    );
}

#[test]
fn a_settings_reload_keeps_the_key_of_the_field_being_typed_into() {
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(M::FieldInput(1, BandField::Gain, "-6".into()));
    let typed_into = key(&app, 1, BandField::Gain);
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let _ = app.handle_player_settings_loaded(
        nokkvi_data::types::player_settings::LivePlayerSettings {
            eq_mode: EqMode::Parametric,
            eq_bands: vec![peak(90.0, 1.0, 1.0), peak(900.0, 2.0, 1.0)],
            ..nokkvi_data::types::player_settings::LivePlayerSettings::default()
        },
    );
    assert_eq!(key(&app, 1, BandField::Gain), typed_into);
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

#[test]
fn an_import_names_the_next_profile_save_until_the_list_changes() {
    let imported = vec![
        EqBand::new(EqBandKind::LowShelf, 105.0, 6.5, 0.7),
        peak(220.0, -3.1, 0.85),
    ];
    let import = || M::ImportLoaded(Ok(("HD 650 ParametricEQ.txt".into(), imported.clone())));

    // Import, Save: the name field starts with the file's name.
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(import());
    let _ = app.handle_eq_modal(M::SavePreset);
    assert_eq!(app.eq_modal.save_name, "HD 650 ParametricEQ");
    // Cancelled, it is still offered.
    let _ = app.handle_eq_modal(M::CancelSave);
    let _ = app.handle_eq_modal(M::SavePreset);
    assert_eq!(app.eq_modal.save_name, "HD 650 ParametricEQ");
    // Saved, it has served.
    let _ = app.handle_eq_modal(M::SavePresetConfirm);
    assert_eq!(app.eq_modal.profiles.len(), 1);
    let _ = app.handle_eq_modal(M::SavePreset);
    assert!(app.eq_modal.save_name.is_empty());
    let _ = app.handle_eq_modal(M::CancelSave);

    // An edit drops it, and editing back doesn't bring it back.
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(import());
    let _ = app.handle_eq_modal(M::ToggleBand(0));
    assert!(app.eq_modal.imported.is_none());
    let _ = app.handle_eq_modal(M::ToggleBand(0));
    let _ = app.handle_eq_modal(M::SavePreset);
    assert!(app.eq_modal.save_name.is_empty());

    // Selecting a profile drops it.
    let mut app = parametric_app(&three_bands());
    app.eq_modal.profiles = vec![profile("Speakers", &speakers())];
    let _ = app.handle_eq_modal(import());
    let _ = app.handle_eq_modal(M::ProfileSelected(0));
    assert!(app.eq_modal.imported.is_none());

    // The Graphic view's preset Save never takes it.
    let mut app = parametric_app(&three_bands());
    let _ = app.handle_eq_modal(import());
    let _ = app.handle_eq_modal(M::ModeSelected(EqMode::Graphic));
    let _ = app.handle_eq_modal(M::SavePreset);
    assert!(app.eq_modal.save_name.is_empty());
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
    assert!(app.eq_modal.imported.is_none());
}
