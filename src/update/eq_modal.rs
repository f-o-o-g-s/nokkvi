use iced::Task;

use crate::{Message, Nokkvi, widgets::EqModalMessage};

impl Nokkvi {
    pub(crate) fn handle_eq_modal(&mut self, msg: EqModalMessage) -> Task<Message> {
        match msg {
            EqModalMessage::Open => {
                self.eq_modal.open = true;
                Task::none()
            }
            EqModalMessage::Close => {
                self.eq_modal.open = false;
                self.eq_modal.save_mode = false;
                Task::none()
            }
            EqModalMessage::Toggle => {
                self.eq_modal.open = !self.eq_modal.open;
                if !self.eq_modal.open {
                    self.eq_modal.save_mode = false;
                }
                Task::none()
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
                    "Failed to save EQ state",
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
                        "Failed to save EQ gains",
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
                        "Failed to save EQ preset",
                    )
                } else {
                    Task::none()
                }
            }
            EqModalMessage::ResetAll => {
                let gains = nokkvi_data::audio::eq::PRESET_FLAT;
                self.playback.eq_state.set_all_gains(&gains);

                self.shell_fire_and_forget_task(
                    move |shell| async move { shell.settings().set_eq_gains(gains).await },
                    "EQ Reset".to_string(),
                    "Failed to save EQ reset",
                )
            }
            EqModalMessage::SavePreset => {
                self.eq_modal.save_mode = true;
                self.eq_modal.save_name.clear();
                Task::none()
            }
            EqModalMessage::SavePresetNameChanged(name) => {
                self.eq_modal.save_name = name;
                Task::none()
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
                    "Failed to save custom preset",
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
                    "Failed to delete custom preset",
                )
            }
        }
    }
}
