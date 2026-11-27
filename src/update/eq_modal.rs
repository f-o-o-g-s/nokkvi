//! The equalizer modal's handler: the Graphic sliders and presets, and the
//! Parametric band editor (view switch, band edits, typed fields, import and
//! export).
//!
//! Every edit goes to `self.playback.eq_state` at once (that is what plays);
//! the band list is persisted on discrete edits and at the end of a drag, not
//! on every drag or keystroke (each persist rewrites config.toml and redb).

use std::path::Path;

use iced::Task;
use nokkvi_data::{
    audio::eq::{EQ_MAX_BANDS, EqBand, EqBandKind, EqMode, EqProfile, apo, graphic_bands},
    utils::noun::Noun,
};
use tracing::{error, info};

use crate::{
    Message, Nokkvi,
    update::{components::session_expired_message, modals::ActiveModal},
    widgets::{
        EqModalMessage,
        eq_modal::ImportedProfile,
        eq_parametric::{
            BandField, FieldEdit, FieldKey, adjacent_field, field_at, field_enabled, focus_field,
            nudged, parse_value, reveal_row, with_value,
        },
    },
};

/// The band Add band puts in: a peak at 1 kHz, flat, about an octave and a
/// half wide.
const NEW_BAND: EqBand = EqBand::new(EqBandKind::Peak, 1_000.0, 0.0, 1.0);

/// Largest profile file Import reads. AutoEq's are under 1 KiB; the cap
/// stops an accidental pick of something huge before it is read in.
const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

impl Nokkvi {
    /// The band-table key `key` is, while the EQ modal is on top showing the
    /// Parametric view: Tab / Shift+Tab, or a bare Up / Down. The key route
    /// (`handle_raw_key_event`) takes these ahead of the text-field guard,
    /// because a focused field captures the arrows.
    pub(crate) fn eq_table_key(
        &self,
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> Option<FieldKey> {
        use iced::keyboard::{Key, key::Named};
        if !self.modal_is_top(ActiveModal::Eq)
            || self.playback.eq_state.mode() != EqMode::Parametric
            || modifiers.control()
            || modifiers.alt()
            || modifiers.logo()
        {
            return None;
        }
        match key {
            Key::Named(Named::Tab) => Some(if modifiers.shift() {
                FieldKey::Previous
            } else {
                FieldKey::Next
            }),
            Key::Named(Named::ArrowUp) if !modifiers.shift() => Some(FieldKey::Up),
            Key::Named(Named::ArrowDown) if !modifiers.shift() => Some(FieldKey::Down),
            Key::Named(_) | Key::Character(_) | Key::Unidentified => None,
        }
    }

    /// Ask which widget has focus, then hand `key` to the band table with it.
    pub(crate) fn eq_table_key_task(key: FieldKey) -> Task<Message> {
        crate::widgets::focused_widget_id()
            .map(move |focused| Message::EqModal(EqModalMessage::FieldKey(focused, key)))
    }

    fn eq_bands(&self) -> Vec<EqBand> {
        self.playback.eq_state.parametric_bands().to_vec()
    }

    /// Persist through the settings service; a failure toasts, a success
    /// toasts `success` when given and is silent otherwise.
    fn persist_eq<F, Fut>(
        &self,
        action: F,
        success: Option<String>,
        error_ctx: &'static str,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        self.shell_task(action, move |result| match result {
            Ok(()) => success.map_or(Message::NoOp, |label| {
                info!(" {label}");
                Message::Toast(crate::app_message::ToastMessage::Push(
                    nokkvi_data::types::toast::Toast::new(
                        label,
                        nokkvi_data::types::toast::ToastLevel::Success,
                    ),
                ))
            }),
            Err(e) => session_expired_message(&e).unwrap_or_else(|| {
                error!(" Failed to {error_ctx}: {e}");
                Message::Toast(crate::app_message::ToastMessage::Push(
                    nokkvi_data::types::toast::Toast::new(
                        format!("Failed to {error_ctx}: {e}"),
                        nokkvi_data::types::toast::ToastLevel::Error,
                    ),
                ))
            }),
        })
    }

    /// Write the band list. The task reads the list when it runs, not when
    /// it was sent, so of two writes in flight the later one never leaves
    /// the older list behind.
    fn persist_eq_bands(&mut self) -> Task<Message> {
        self.eq_modal.bands_unsaved = false;
        let eq = self.playback.eq_state.clone();
        self.persist_eq(
            move |shell| async move {
                let bands = eq.parametric_bands().to_vec();
                shell.settings().set_eq_bands(bands).await
            },
            None,
            "save the equalizer bands",
        )
    }

    /// Write the band list if a live edit left it unsaved.
    fn flush_eq_bands(&mut self) -> Task<Message> {
        if self.eq_modal.bands_unsaved {
            self.persist_eq_bands()
        } else {
            Task::none()
        }
    }

    /// End the field being typed into. Its text already played whenever it
    /// parsed, so committing only lets the field show the stored value again
    /// (text that never parsed is dropped) and writes what is unsaved.
    fn commit_eq_field(&mut self) -> Task<Message> {
        self.eq_modal.edit = None;
        self.flush_eq_bands()
    }

    /// Play `bands`; persisted later (drag release, commit, Close).
    fn set_eq_bands_live(&mut self, bands: &[EqBand]) {
        self.playback.eq_state.set_parametric_bands(bands);
        self.eq_modal.bands_unsaved = true;
    }

    /// Play `bands` and persist them now (a discrete edit).
    fn set_eq_bands_now(&mut self, bands: &[EqBand]) -> Task<Message> {
        self.playback.eq_state.set_parametric_bands(bands);
        self.persist_eq_bands()
    }

    /// The band list with band `index` changed by `edit`, or `None` when the
    /// index is past the list (it shrank under a stale message).
    fn edited_eq_bands(
        &self,
        index: usize,
        edit: impl FnOnce(EqBand) -> EqBand,
    ) -> Option<Vec<EqBand>> {
        let mut bands = self.eq_bands();
        let band = bands.get_mut(index)?;
        *band = edit(*band).clamped();
        Some(bands)
    }

    /// A discrete edit of one band (type, on/off, a wheel step): commit any
    /// typed field, apply, persist, select the band.
    fn edit_eq_band_now(
        &mut self,
        index: usize,
        edit: impl FnOnce(EqBand) -> EqBand,
    ) -> Task<Message> {
        let commit = self.commit_eq_field();
        let Some(bands) = self.edited_eq_bands(index, edit) else {
            return commit;
        };
        self.eq_modal.selected = Some(index);
        let persist = self.set_eq_bands_now(&bands);
        Task::batch([commit, persist])
    }

    /// Append `band` and select it; at the cap, a toast instead.
    fn add_eq_band(&mut self, band: EqBand) -> Task<Message> {
        let mut bands = self.eq_bands();
        if bands.len() >= EQ_MAX_BANDS {
            self.toast_warn(format!("At most {}", Noun::BAND.count(EQ_MAX_BANDS)));
            return Task::none();
        }
        let commit = self.commit_eq_field();
        bands.push(band.clamped());
        let added = bands.len() - 1;
        self.eq_modal.selected = Some(added);
        let persist = self.set_eq_bands_now(&bands);
        Task::batch([commit, persist, reveal_row(added)])
    }

    /// Select band `index` (nothing, past the list). A press on a point or a
    /// row: any typed field lost focus, so it is committed.
    fn select_eq_band(&mut self, index: usize) -> Task<Message> {
        let commit = self.commit_eq_field();
        let len = self.playback.eq_state.parametric_bands().len();
        self.eq_modal.selected = (index < len).then_some(index);
        commit
    }

    /// The typed field and unsaved bands of the Parametric view, written out
    /// as the modal closes.
    fn finish_eq_edits(&mut self) -> Task<Message> {
        self.commit_eq_field()
    }

    fn handle_eq_field_key(
        &mut self,
        focused: Option<iced::widget::Id>,
        key: FieldKey,
    ) -> Task<Message> {
        let mut bands = self.eq_bands();
        let at = focused.as_ref().and_then(|id| field_at(id, bands.len()));
        match key {
            FieldKey::Next | FieldKey::Previous => {
                let commit = self.commit_eq_field();
                match adjacent_field(&bands, at, key == FieldKey::Next) {
                    Some((band, field)) => {
                        self.eq_modal.selected = Some(band);
                        Task::batch([commit, focus_field(band, field)])
                    }
                    None => commit,
                }
            }
            FieldKey::Up | FieldKey::Down => {
                let Some((index, field)) = at else {
                    return Task::none();
                };
                if !field_enabled(&bands[index], field) {
                    return Task::none();
                }
                // Typed text is dropped: the nudge starts from what plays.
                self.eq_modal.edit = None;
                let before = bands[index];
                bands[index] = nudged(before, field, key == FieldKey::Up);
                self.eq_modal.selected = Some(index);
                if bands[index] == before {
                    // At a limit: nothing changes, the field keeps its state.
                    return Task::none();
                }
                // Live only: a held key repeats ~25 times a second. The
                // next commit or Close writes it.
                self.set_eq_bands_live(&bands);
                // The new value re-keys the field (`FieldMounts`), and the
                // fresh widget has no focus: hand it back.
                focus_field(index, field)
            }
        }
    }

    fn handle_eq_field_input(
        &mut self,
        index: usize,
        field: BandField,
        typed: String,
    ) -> Task<Message> {
        let mut bands = self.eq_bands();
        let Some(&band) = bands.get(index) else {
            // The list shrank under the field (a config.toml reload).
            self.eq_modal.edit = None;
            return Task::none();
        };
        if !field_enabled(&band, field) {
            return Task::none();
        }
        let other_field = self
            .eq_modal
            .edit
            .as_ref()
            .is_some_and(|edit| (edit.band, edit.field) != (index, field));
        let commit = if other_field {
            self.commit_eq_field()
        } else {
            Task::none()
        };
        let value = parse_value(field, &typed);
        self.eq_modal.edit = Some(FieldEdit {
            band: index,
            field,
            text: typed,
        });
        self.eq_modal.selected = Some(index);
        if let Some(value) = value {
            bands[index] = with_value(band, field, value);
            self.set_eq_bands_live(&bands);
        }
        commit
    }

    fn handle_eq_field_submit(&mut self, index: usize, field: BandField) -> Task<Message> {
        let commit = self.commit_eq_field();
        let bands = self.eq_bands();
        if index >= bands.len() {
            return commit;
        }
        match adjacent_field(&bands, Some((index, field)), true) {
            Some((band, next)) => {
                self.eq_modal.selected = Some(band);
                Task::batch([commit, focus_field(band, next)])
            }
            None => commit,
        }
    }

    fn handle_eq_remove_band(&mut self, index: usize) -> Task<Message> {
        let mut bands = self.eq_bands();
        if index >= bands.len() {
            return Task::none();
        }
        let commit = self.commit_eq_field();
        bands.remove(index);
        self.eq_modal.selected = match self.eq_modal.selected {
            // Its own row went: the band that slid into its place, or the
            // new last one.
            Some(selected) if selected == index => {
                (!bands.is_empty()).then(|| index.min(bands.len() - 1))
            }
            // A row above it went: it moves up with its band.
            Some(selected) if selected > index => Some(selected - 1),
            other => other.filter(|&selected| selected < bands.len()),
        };
        let persist = self.set_eq_bands_now(&bands);
        Task::batch([commit, persist])
    }

    fn handle_eq_import_loaded(
        &mut self,
        loaded: Result<(String, Vec<EqBand>), String>,
    ) -> Task<Message> {
        let (name, bands) = match loaded {
            Ok(profile) => profile,
            Err(why) => {
                self.toast_error(why);
                return Task::none();
            }
        };
        let eq = &self.playback.eq_state;
        eq.set_parametric_bands(&bands);
        eq.set_mode(EqMode::Parametric);
        eq.set_enabled(true);
        let count = eq.parametric_bands().len();
        // The file replaces the list: nothing typed or selected carries over.
        self.eq_modal.edit = None;
        self.eq_modal.selected = None;
        self.eq_modal.save_mode = false;
        // The next profile Save starts from the file's name, while the list
        // is still the file's.
        self.eq_modal.imported = Some(ImportedProfile {
            name: profile_name_of(&name),
            bands: eq.parametric_bands().to_vec(),
        });
        let save_bands = self.persist_eq_bands();
        let save_layout = self.persist_eq(
            move |shell| async move {
                let settings = shell.settings();
                settings.set_eq_mode(EqMode::Parametric).await?;
                settings.set_eq_enabled(true).await
            },
            Some(format!(
                "Imported {} from {name}",
                Noun::FILTER.count(count)
            )),
            "save the imported equalizer profile",
        );
        Task::batch([save_bands, save_layout])
    }

    /// Save in the Parametric view: the name prompt, starting from the last
    /// imported file's name while the list is still that file's. An empty
    /// list has nothing to save.
    fn start_saving_eq_profile(&mut self) -> Task<Message> {
        // The press took focus from a typed field: commit it first.
        let commit = self.commit_eq_field();
        if self.playback.eq_state.parametric_bands().is_empty() {
            self.toast_warn("No bands to save");
            return commit;
        }
        self.eq_modal.save_mode = true;
        self.eq_modal.save_name = self
            .eq_modal
            .imported
            .as_ref()
            .map(|imported| imported.name.clone())
            .unwrap_or_default();
        Task::batch([commit, focus_save_name()])
    }

    /// The Parametric prompt's OK: the name must be new among the saved
    /// profiles (any case); the band list that plays is saved under it.
    fn confirm_eq_profile_save(&mut self) -> Task<Message> {
        let name = self.eq_modal.save_name.trim().to_string();
        if name.is_empty() {
            self.toast_warn("Profile name cannot be empty");
            return Task::none();
        }
        let name_lower = name.to_lowercase();
        if self
            .eq_modal
            .profiles
            .iter()
            .any(|profile| profile.name.to_lowercase() == name_lower)
        {
            self.toast_warn(format!("A profile named '{name}' already exists"));
            return Task::none();
        }
        let bands = self.eq_bands();
        if bands.is_empty() {
            self.toast_warn("No bands to save");
            return Task::none();
        }
        self.eq_modal.profiles.push(EqProfile {
            name: name.clone(),
            bands: bands.clone(),
        });
        self.eq_modal.save_mode = false;
        self.eq_modal.save_name.clear();
        // The import's name has served.
        self.eq_modal.imported = None;
        self.persist_eq(
            move |shell| async move { shell.settings().save_eq_profile(name, bands).await },
            Some("Profile saved".to_owned()),
            "save the equalizer profile",
        )
    }

    /// A saved profile picked: its bands replace the list and play (the
    /// equalizer turns on, as a preset does), nothing typed or selected
    /// carries over.
    fn select_eq_profile(&mut self, index: usize) -> Task<Message> {
        let Some(profile) = self.eq_modal.profiles.get(index).cloned() else {
            return Task::none();
        };
        self.eq_modal.edit = None;
        self.eq_modal.selected = None;
        self.eq_modal.imported = None;
        let eq = &self.playback.eq_state;
        eq.set_parametric_bands(&profile.bands);
        eq.set_enabled(true);
        let save_bands = self.persist_eq_bands();
        let save_enabled = self.persist_eq(
            |shell| async move { shell.settings().set_eq_enabled(true).await },
            Some(format!("Profile: {}", profile.name)),
            "save the equalizer profile choice",
        );
        Task::batch([save_bands, save_enabled])
    }

    /// Delete a saved profile. The band list stays as it is and keeps
    /// playing (the Graphic preset delete flattens the sliders; this doesn't).
    fn delete_eq_profile(&mut self, index: usize) -> Task<Message> {
        if index >= self.eq_modal.profiles.len() {
            return Task::none();
        }
        let name = self.eq_modal.profiles.remove(index).name;
        self.persist_eq(
            move |shell| async move { shell.settings().delete_eq_profile(index).await },
            Some(format!("Deleted profile '{name}'")),
            "delete the equalizer profile",
        )
    }

    fn handle_eq_export(&mut self) -> Task<Message> {
        let bands = self.eq_bands();
        if bands.is_empty() {
            self.toast_warn("No bands to export");
            return Task::none();
        }
        let commit = self.commit_eq_field();
        let count = bands.len().min(EQ_MAX_BANDS);
        let text = apo::write(&bands);
        let export = Task::future(export_eq_profile(text, count)).and_then(|written| {
            Task::done(Message::EqModal(EqModalMessage::ExportWritten(written)))
        });
        Task::batch([commit, export])
    }

    /// Handle one EQ modal message, then bring the band table's text fields
    /// up to date with what it changed (`sync_eq_fields`).
    pub(crate) fn handle_eq_modal(&mut self, msg: EqModalMessage) -> Task<Message> {
        let task = self.handle_eq_modal_message(msg);
        self.sync_eq_fields();
        task
    }

    /// Re-key every band-table field whose text changed from outside it (see
    /// `FieldMounts`). Call it wherever the band list or the field being
    /// typed into changes outside `handle_eq_modal`.
    pub(crate) fn sync_eq_fields(&mut self) {
        let bands = self.playback.eq_state.parametric_bands();
        self.eq_modal.sync_fields(&bands);
    }

    fn handle_eq_modal_message(&mut self, msg: EqModalMessage) -> Task<Message> {
        match msg {
            EqModalMessage::Open => {
                self.eq_modal.open = true;
                Task::none()
            }
            EqModalMessage::Close => {
                self.eq_modal.open = false;
                self.eq_modal.save_mode = false;
                self.finish_eq_edits()
            }
            EqModalMessage::Toggle => {
                self.eq_modal.open = !self.eq_modal.open;
                if self.eq_modal.open {
                    Task::none()
                } else {
                    self.eq_modal.save_mode = false;
                    self.finish_eq_edits()
                }
            }
            EqModalMessage::ToggleEnabled => {
                let current = self.playback.eq_state.is_enabled();
                self.playback.eq_state.set_enabled(!current);

                // Persist to storage
                let enabled = !current;
                let msg = if enabled { "EQ Enabled" } else { "EQ Disabled" };
                self.shell_fire_and_forget_task(
                    move |shell| async move { shell.settings().set_eq_enabled(enabled).await },
                    msg.to_string(),
                    "save the equalizer state",
                )
            }
            EqModalMessage::GainChanged(band, gain) => {
                if band < 10 {
                    self.playback.eq_state.set_band_gain(band, gain);

                    // Read back all gains for persistence
                    let mut gains = [0.0; 10];
                    for (i, g) in gains.iter_mut().enumerate() {
                        *g = self.playback.eq_state.get_band_gain(i);
                    }

                    self.shell_fire_and_forget_task(
                        move |shell| async move { shell.settings().set_eq_gains(gains).await },
                        String::new(), // Silent
                        "save the equalizer gains",
                    )
                } else {
                    Task::none()
                }
            }
            EqModalMessage::PresetSelected(choice) => {
                let (gains, preset_name) = match &choice {
                    crate::widgets::PresetChoice::Builtin(idx) => {
                        let preset = nokkvi_data::audio::eq::BUILTIN_PRESETS.get(*idx);
                        (
                            preset.map(|p| p.gains),
                            preset.map_or("Unknown", |p| p.name).to_string(),
                        )
                    }
                    crate::widgets::PresetChoice::Custom(idx) => {
                        let preset = self.eq_modal.custom_presets.get(*idx);
                        (
                            preset.map(|p| p.gains),
                            preset.map(|p| p.name.clone()).unwrap_or_default(),
                        )
                    }
                };

                if let Some(gains) = gains {
                    self.playback.eq_state.set_all_gains(&gains);
                    self.playback.eq_state.set_enabled(true);
                    self.shell_fire_and_forget_task(
                        move |shell| async move {
                            shell.settings().set_eq_gains(gains).await?;
                            shell.settings().set_eq_enabled(true).await
                        },
                        format!("Preset: {preset_name}"),
                        "save the equalizer preset",
                    )
                } else {
                    Task::none()
                }
            }
            EqModalMessage::ResetAll if self.playback.eq_state.mode() == EqMode::Parametric => {
                // Every band to 0 dB, the bands kept; a pass filter has no
                // gain and stays as it is.
                let commit = self.commit_eq_field();
                let bands: Vec<EqBand> = self
                    .eq_bands()
                    .into_iter()
                    .map(|band| {
                        if band.kind.has_gain() {
                            EqBand { gain: 0.0, ..band }
                        } else {
                            band
                        }
                    })
                    .collect();
                self.playback.eq_state.set_parametric_bands(&bands);
                let persist = self.persist_eq_bands();
                self.toast_success("EQ Reset");
                Task::batch([commit, persist])
            }
            EqModalMessage::ResetAll => {
                let gains = nokkvi_data::audio::eq::PRESET_FLAT;
                self.playback.eq_state.set_all_gains(&gains);

                self.shell_fire_and_forget_task(
                    move |shell| async move { shell.settings().set_eq_gains(gains).await },
                    "EQ Reset".to_string(),
                    "save the equalizer reset",
                )
            }
            EqModalMessage::SavePreset if self.playback.eq_state.mode() == EqMode::Parametric => {
                self.start_saving_eq_profile()
            }
            EqModalMessage::SavePreset => {
                self.eq_modal.save_mode = true;
                self.eq_modal.save_name.clear();
                focus_save_name()
            }
            EqModalMessage::SavePresetNameChanged(name) => {
                self.eq_modal.save_name = name;
                Task::none()
            }
            EqModalMessage::SavePresetConfirm
                if self.playback.eq_state.mode() == EqMode::Parametric =>
            {
                self.confirm_eq_profile_save()
            }
            EqModalMessage::SavePresetConfirm => {
                let name = self.eq_modal.save_name.trim().to_string();
                if name.is_empty() {
                    self.toast_warn("Preset name cannot be empty".to_string());
                    return Task::none();
                }

                // Reject duplicate names (custom presets + builtins)
                let name_lower = name.to_lowercase();
                let duplicate_custom = self
                    .eq_modal
                    .custom_presets
                    .iter()
                    .any(|p| p.name.to_lowercase() == name_lower);
                let duplicate_builtin = nokkvi_data::audio::eq::BUILTIN_PRESETS
                    .iter()
                    .any(|p| p.name.to_lowercase() == name_lower);
                if duplicate_custom || duplicate_builtin {
                    self.toast_warn(format!("A preset named '{name}' already exists"));
                    return Task::none();
                }

                // Read current gains
                let mut gains = [0.0; 10];
                for (i, g) in gains.iter_mut().enumerate() {
                    *g = self.playback.eq_state.get_band_gain(i);
                }

                // Add to local cache
                self.eq_modal
                    .custom_presets
                    .push(nokkvi_data::audio::eq::CustomEqPreset {
                        name: name.clone(),
                        gains,
                    });

                // Exit save mode
                self.eq_modal.save_mode = false;
                self.eq_modal.save_name.clear();

                self.shell_fire_and_forget_task(
                    move |shell| async move {
                        shell.settings().save_custom_eq_preset(name, gains).await
                    },
                    "Preset saved".to_string(),
                    "save the custom preset",
                )
            }
            EqModalMessage::CancelSave => {
                self.eq_modal.save_mode = false;
                self.eq_modal.save_name.clear();
                Task::none()
            }
            EqModalMessage::DeletePreset(idx) => {
                let name = self
                    .eq_modal
                    .custom_presets
                    .get(idx)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();

                // Remove from local cache
                if idx < self.eq_modal.custom_presets.len() {
                    self.eq_modal.custom_presets.remove(idx);
                }

                // Reset gains to flat after deleting the active preset
                let gains = nokkvi_data::audio::eq::PRESET_FLAT;
                self.playback.eq_state.set_all_gains(&gains);

                self.shell_fire_and_forget_task(
                    move |shell| async move {
                        shell.settings().delete_custom_eq_preset(idx).await?;
                        shell.settings().set_eq_gains(gains).await
                    },
                    format!("Deleted preset '{name}'"),
                    "delete the custom preset",
                )
            }
            EqModalMessage::ProfileSelected(index) => self.select_eq_profile(index),
            EqModalMessage::DeleteProfile(index) => self.delete_eq_profile(index),
            EqModalMessage::ModeSelected(mode) => {
                if mode == self.playback.eq_state.mode() {
                    return Task::none();
                }
                let commit = self.commit_eq_field();
                // The name prompt is one layout's (a preset or a profile).
                self.eq_modal.save_mode = false;
                self.eq_modal.save_name.clear();
                self.playback.eq_state.set_mode(mode);
                let persist = self.persist_eq(
                    move |shell| async move { shell.settings().set_eq_mode(mode).await },
                    None,
                    "save the equalizer layout",
                );
                Task::batch([commit, persist])
            }
            EqModalMessage::SelectBand(index) => self.select_eq_band(index),
            EqModalMessage::PointPressed(index) => {
                // The row may sit out of the table's view; a row press never
                // scrolls (the row would move under the cursor).
                let select = self.select_eq_band(index);
                if self.eq_modal.selected == Some(index) {
                    Task::batch([select, reveal_row(index)])
                } else {
                    select
                }
            }
            EqModalMessage::BandDragged(index, freq, gain) => {
                let Some(bands) = self.edited_eq_bands(index, |band| EqBand {
                    freq,
                    gain: if band.kind.has_gain() {
                        gain
                    } else {
                        band.gain
                    },
                    ..band
                }) else {
                    return Task::none();
                };
                if self
                    .eq_modal
                    .edit
                    .as_ref()
                    .is_some_and(|edit| edit.band == index)
                {
                    self.eq_modal.edit = None;
                }
                self.eq_modal.selected = Some(index);
                self.set_eq_bands_live(&bands);
                Task::none()
            }
            EqModalMessage::BandDragEnded => self.flush_eq_bands(),
            EqModalMessage::BandQStepped(index, q) => Task::batch([
                self.edit_eq_band_now(index, |band| EqBand { q, ..band }),
                reveal_row(index),
            ]),
            EqModalMessage::AddBand => self.add_eq_band(NEW_BAND),
            EqModalMessage::AddBandAt(freq, gain) => self.add_eq_band(EqBand {
                freq,
                gain,
                ..NEW_BAND
            }),
            EqModalMessage::RemoveBand(index) => self.handle_eq_remove_band(index),
            EqModalMessage::ToggleBand(index) => self.edit_eq_band_now(index, |band| EqBand {
                enabled: !band.enabled,
                ..band
            }),
            EqModalMessage::BandKindSelected(index, kind) => {
                self.edit_eq_band_now(index, |band| EqBand { kind, ..band })
            }
            EqModalMessage::FieldInput(index, field, typed) => {
                self.handle_eq_field_input(index, field, typed)
            }
            EqModalMessage::FieldSubmit(index, field) => self.handle_eq_field_submit(index, field),
            EqModalMessage::FieldKey(focused, key) => self.handle_eq_field_key(focused, key),
            EqModalMessage::CopyFromGraphic => {
                let commit = self.commit_eq_field();
                let bands = graphic_bands(&self.playback.eq_state.graphic_gains());
                self.eq_modal.selected = None;
                let persist = self.set_eq_bands_now(&bands);
                Task::batch([commit, persist])
            }
            EqModalMessage::Import => Task::future(pick_eq_profile()).and_then(|loaded| {
                Task::done(Message::EqModal(EqModalMessage::ImportLoaded(loaded)))
            }),
            EqModalMessage::ImportLoaded(loaded) => self.handle_eq_import_loaded(loaded),
            EqModalMessage::Export => self.handle_eq_export(),
            EqModalMessage::ExportWritten(Ok((name, count))) => {
                self.toast_success(format!("Exported {} to {name}", Noun::FILTER.count(count)));
                Task::none()
            }
            EqModalMessage::ExportWritten(Err(why)) => {
                self.toast_error(why);
                Task::none()
            }
        }
    }
}

/// Put the caret in the save prompt's name field as the prompt opens.
fn focus_save_name() -> Task<Message> {
    crate::widgets::focus_text_input(crate::widgets::eq_modal::SAVE_NAME_INPUT_ID)
}

/// The file name `path` ends in, for a toast.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The name an imported file offers its profile: the file name without its
/// extension (`HD 650 ParametricEQ.txt` -> `HD 650 ParametricEQ`).
pub(crate) fn profile_name_of(file_name: &str) -> String {
    Path::new(file_name).file_stem().map_or_else(
        || file_name.to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

/// The bands of a profile file's `text` (or why it can't be read), named for
/// the toast: `(file name, bands)` or `Couldn't import <file name>: <why>`.
/// Never a partial list: `apo::parse` refuses the whole file.
pub(crate) fn profile_from_text(
    name: &str,
    text: Result<String, String>,
) -> Result<(String, Vec<EqBand>), String> {
    text.and_then(|text| apo::parse(&text).map_err(|why| why.to_string()))
        .map(|bands| (name.to_owned(), bands))
        .map_err(|why| format!("Couldn't import {name}: {why}"))
}

/// Import…: the native (xdg-portal) picker, then the size guard, the read and
/// the parse, all off the UI thread. `None` when the dialog was dismissed.
async fn pick_eq_profile() -> Option<Result<(String, Vec<EqBand>), String>> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .set_title("Import equalizer profile")
        // Both cases: rfd's xdg-portal backend forwards these as literal,
        // case-sensitive globs.
        .add_filter("Equalizer profiles", &["txt", "TXT"])
        .add_filter("All files", &["*"])
        .pick_file()
        .await
    else {
        info!(
            "EQ profile picker returned no file: user cancelled, or (if no dialog ever \
             appeared) a FileChooser portal implementation may be missing"
        );
        return None;
    };
    let path = handle.path().to_path_buf();
    let name = file_name(&path);
    Some(profile_from_text(&name, read_profile(&path).await))
}

/// The text of the profile at `path`, refused over [`MAX_PROFILE_BYTES`]
/// before it is read in.
async fn read_profile(path: &Path) -> Result<String, String> {
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.len() > MAX_PROFILE_BYTES => {
            return Err("the file is over 1 MiB".to_owned());
        }
        Ok(_) => {}
        Err(e) => return Err(format!("couldn't read it ({e})")),
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| format!("couldn't read it ({e})"))?;
    String::from_utf8(bytes).map_err(|_| "it isn't a UTF-8 text file".to_owned())
}

/// Export…: the native save dialog, then the write, off the UI thread.
/// `None` when the dialog was dismissed.
async fn export_eq_profile(text: String, count: usize) -> Option<Result<(String, usize), String>> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .set_title("Export equalizer profile")
        .set_file_name("nokkvi-eq.txt")
        .add_filter("Equalizer profiles", &["txt", "TXT"])
        .save_file()
        .await
    else {
        info!("EQ profile save dialog returned no file: user cancelled, or no portal");
        return None;
    };
    let path = handle.path().to_path_buf();
    let name = file_name(&path);
    Some(
        tokio::fs::write(&path, text)
            .await
            .map(|()| (name.clone(), count))
            .map_err(|e| format!("Couldn't export to {name}: {e}")),
    )
}

#[cfg(test)]
mod tests {
    use super::{profile_from_text, profile_name_of};

    #[test]
    fn an_imported_file_offers_its_name_without_the_extension() {
        assert_eq!(
            profile_name_of("HD 650 ParametricEQ.txt"),
            "HD 650 ParametricEQ"
        );
        assert_eq!(profile_name_of("speakers.TXT"), "speakers");
        assert_eq!(profile_name_of("v1.2 tuning.txt"), "v1.2 tuning");
        assert_eq!(profile_name_of("no extension"), "no extension");
    }

    #[test]
    fn a_profile_text_imports_whole_or_says_why_with_the_file_name() {
        let text = "Preamp: -6.2 dB\n\
                    Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70\n\
                    Filter 2: ON PK Fc 220 Hz Gain -3.1 dB Q 0.85\n";
        let (name, bands) =
            profile_from_text("HD 650.txt", Ok(text.to_owned())).expect("a valid profile");
        assert_eq!(name, "HD 650.txt");
        assert_eq!(bands.len(), 2);

        let bad = "Filter 1: ON BP Fc 1000 Hz Q 1\n";
        let why = profile_from_text("bad.txt", Ok(bad.to_owned())).expect_err("refused");
        assert!(why.starts_with("Couldn't import bad.txt: line 1:"), "{why}");

        let why = profile_from_text("big.txt", Err("the file is over 1 MiB".into()))
            .expect_err("refused");
        assert_eq!(why, "Couldn't import big.txt: the file is over 1 MiB");
    }
}
