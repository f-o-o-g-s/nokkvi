//! MilkDrop preset picker handler: open over the running visualizer, preview
//! the centered row live, choose (and lock) it, or close back to the preset
//! that was on screen.
//!
//! State lives on `Nokkvi.milkdrop.picker`. While it is open the picker holds
//! the preset lock (no timer or track-change switch) and owns what loads: the
//! tick calls [`Nokkvi::milkdrop_picker_sync_preview`] instead of drawing a
//! random preset, and a failed preview is not followed by a random one.
//! Previews run one build at a time; a list that moved on while a build was
//! in flight catches up on the tick after it lands.

use iced::Task;
use tracing::{debug, info};

use crate::{
    Nokkvi,
    app_message::Message,
    state::MILKDROP_HISTORY_CAP,
    widgets::milkdrop_picker::{
        MILKDROP_PICKER_SEARCH_INPUT_ID, MilkdropPickerMessage, MilkdropPickerState,
    },
};

impl Nokkvi {
    pub(crate) fn handle_milkdrop_picker(&mut self, msg: MilkdropPickerMessage) -> Task<Message> {
        match msg {
            MilkdropPickerMessage::Close => self.close_milkdrop_picker(),
            MilkdropPickerMessage::SearchChanged(query) => {
                if let Some(state) = self.milkdrop.picker.as_mut() {
                    state.search_query = query;
                    state.refilter(&self.milkdrop.library);
                }
                self.milkdrop_picker_sync_preview()
            }
            MilkdropPickerMessage::SlotListUp => {
                if let Some(state) = self.milkdrop.picker.as_mut() {
                    state.slot_list.move_up(state.filtered.len());
                }
                self.milkdrop_picker_sync_preview()
            }
            MilkdropPickerMessage::SlotListDown => {
                if let Some(state) = self.milkdrop.picker.as_mut() {
                    state.slot_list.move_down(state.filtered.len());
                }
                self.milkdrop_picker_sync_preview()
            }
            MilkdropPickerMessage::SlotListSetOffset(offset) => {
                if let Some(state) = self.milkdrop.picker.as_mut() {
                    state.slot_list.set_offset(offset, state.filtered.len());
                }
                self.milkdrop_picker_sync_preview()
            }
            MilkdropPickerMessage::ClickItem(index) => {
                if let Some(state) = self.milkdrop.picker.as_mut() {
                    state.slot_list.set_offset(index, state.filtered.len());
                }
                self.choose_milkdrop_picker_center()
            }
            MilkdropPickerMessage::ActivateCenter => self.choose_milkdrop_picker_center(),
            MilkdropPickerMessage::ToggleFavorite(name) => {
                let favorite = self.milkdrop.library.toggle_favorite(&name);
                debug!(preset = %name, favorite, "milkdrop picker: favorite toggled");
                self.milkdrop_save_curation();
                // Unfavoriting under the Favorites chip drops the row.
                if let Some(state) = self.milkdrop.picker.as_mut()
                    && state.favorites_only
                {
                    state.refilter(&self.milkdrop.library);
                }
                refocus_search()
            }
            MilkdropPickerMessage::ToggleHidden(name) => {
                if self.milkdrop.library.is_hidden(&name) {
                    self.milkdrop.library.unhide(&name);
                    info!(preset = %name, "milkdrop: preset unhidden");
                } else {
                    self.milkdrop.library.hide(&name);
                    info!(preset = %name, "milkdrop: preset hidden");
                    // A hidden preset never belongs in Previous's history.
                    self.milkdrop.history.retain(|n| *n != name);
                }
                self.milkdrop_save_curation();
                refocus_search()
            }
            MilkdropPickerMessage::ToggleFavoritesOnly => {
                if let Some(state) = self.milkdrop.picker.as_mut() {
                    state.favorites_only = !state.favorites_only;
                    state.refilter(&self.milkdrop.library);
                }
                Task::batch([self.milkdrop_picker_sync_preview(), refocus_search()])
            }
        }
    }

    /// Open the picker over the preset on screen (the Choose Preset control).
    /// Works paused too: the choice then loads once music plays.
    pub(crate) fn open_milkdrop_picker(&mut self) -> Task<Message> {
        if !self.milkdrop_mode_active() {
            debug!("milkdrop: Choose Preset ignored outside MilkDrop mode");
            self.toast_info("Switch the visualizer to MilkDrop to choose a preset");
            return Task::none();
        }
        if self.milkdrop.picker.is_some() {
            return Task::none();
        }
        if self.milkdrop.library.is_empty() {
            self.toast_warn("MilkDrop: no presets found");
            return Task::none();
        }
        // End a spin rather than leave it cruising behind the picker (the
        // other modals leave one running; Enter and Escape reach the modal
        // first either way).
        self.cancel_roulette_restoring_offset();
        let state = MilkdropPickerState::new(
            &self.milkdrop.library,
            self.milkdrop.current.clone(),
            self.milkdrop.locked,
        );
        self.milkdrop.picker = Some(state);
        // Hold the preset while the user picks: no timer, no track change.
        self.milkdrop.locked = true;
        self.milkdrop.next_switch_at = None;
        refocus_search()
    }

    /// Load the centered row if it is not what is on screen already. One build
    /// at a time: while one is in flight nothing starts, and the tick calls
    /// this again once it lands. Idle while MilkDrop is not running (nobody
    /// would see it); a row that failed to load is skipped.
    pub(crate) fn milkdrop_picker_sync_preview(&mut self) -> Task<Message> {
        let Some(target) = self
            .milkdrop
            .picker
            .as_ref()
            .and_then(|p| p.centered())
            .map(str::to_string)
        else {
            return Task::none();
        };
        if !self.milkdrop_is_running()
            || self.milkdrop.build_in_flight.is_some()
            || self.milkdrop.current.as_deref() == Some(target.as_str())
            || self.milkdrop.library.is_broken(&target)
        {
            return Task::none();
        }
        debug!(preset = %target, "milkdrop picker: previewing");
        self.milkdrop.consecutive_failures = 0;
        self.milkdrop_load(target)
    }

    /// Enter / a row click: keep the centered preset and lock it. The preset
    /// that was on screen at open joins Previous's history.
    fn choose_milkdrop_picker_center(&mut self) -> Task<Message> {
        let Some(name) = self
            .milkdrop
            .picker
            .as_ref()
            .and_then(|p| p.centered())
            .map(str::to_string)
        else {
            return Task::none();
        };
        if self.milkdrop.library.is_broken(&name) {
            self.toast_warn(format!("MilkDrop: {name} could not be loaded"));
            return Task::none();
        }
        let Some(state) = self.milkdrop.picker.take() else {
            return Task::none();
        };
        info!(preset = %name, "milkdrop: preset chosen");
        if let Some(original) = state.original
            && original != name
            && self.milkdrop_can_return_to(&original)
        {
            self.milkdrop.history.push(original);
            if self.milkdrop.history.len() > MILKDROP_HISTORY_CAP {
                self.milkdrop.history.remove(0);
            }
        }
        self.milkdrop.locked = true;
        self.milkdrop.next_switch_at = None;
        self.milkdrop.consecutive_failures = 0;
        self.toast_info("MilkDrop: preset locked");
        let task = if self.milkdrop.current.as_deref() == Some(name.as_str()) {
            Task::none()
        } else {
            self.milkdrop_load(name)
        };
        // Still on its way (a preview mid-build, or chosen while paused):
        // remember it, so a failed build releases the lock.
        let generation = self.milkdrop.generation;
        self.milkdrop.chosen_generation =
            (self.milkdrop.announced_generation != generation).then_some(generation);
        task
    }

    /// Close without choosing: back to the preset that was on screen at open
    /// (unless it was hidden meanwhile: then the rotation moves on), and the
    /// lock goes back to how it was.
    fn close_milkdrop_picker(&mut self) -> Task<Message> {
        let Some(state) = self.milkdrop.picker.take() else {
            return Task::none();
        };
        self.milkdrop.locked = state.was_locked;
        self.milkdrop.next_switch_at = None;
        self.milkdrop.consecutive_failures = 0;
        match state.original {
            Some(original)
                if self.milkdrop.current.as_deref() == Some(original.as_str())
                    && self.milkdrop_can_return_to(&original) =>
            {
                Task::none()
            }
            Some(original) if self.milkdrop_can_return_to(&original) => {
                self.milkdrop_load(original)
            }
            // The original is gone (hidden since, or the picker opened with
            // nothing on screen): drop whatever was previewed and let the
            // rotation move on. A preview still building is superseded.
            Some(_) | None => {
                if !self.milkdrop.library.has_eligible() {
                    // Everything is hidden: give the panel back to the cover.
                    self.milkdrop_release();
                    self.milkdrop.empty_warned = true;
                    self.toast_warn("MilkDrop: every preset is hidden");
                } else if self.milkdrop.current.is_some() {
                    self.milkdrop_forget_current();
                }
                Task::none()
            }
        }
    }

    /// Drop the picker without touching what loads (the renderer is being
    /// released anyway): leaving the mode, stop, logout, window close.
    pub(crate) fn milkdrop_picker_discard(&mut self) {
        if let Some(state) = self.milkdrop.picker.take() {
            self.milkdrop.locked = state.was_locked;
        }
    }
}

/// Keep typing in the search field: a click on a row's heart / eye or the
/// Favorites chip takes focus off it, and Backspace would then step the list.
fn refocus_search() -> Task<Message> {
    iced::widget::operation::focus(MILKDROP_PICKER_SEARCH_INPUT_ID)
}
