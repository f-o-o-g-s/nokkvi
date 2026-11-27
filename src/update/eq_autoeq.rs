//! The EQ modal's AutoEq panel: open it, search the list, pick a headphone,
//! load its profile.
//!
//! The panel is the user's request: nothing reaches GitHub before it opens.
//! The first open of a session asks the backend for the list (the cached copy
//! while it is fresh, else a download); later opens show the session's copy
//! at once. Every request goes through `shell_task` and carries the
//! [`StaleDropGen`](crate::state::StaleDropGen) on `EqModalState`, bumped by
//! every dispatch and by every open, so a result for a panel since closed,
//! reopened or asked again is dropped. A loaded profile goes in exactly as an
//! imported file does (`Nokkvi::load_eq_profile`).

use iced::{
    Task,
    event::Status,
    keyboard::{Key, Modifiers, key::Named},
};
use nokkvi_data::{
    audio::eq::{EqBand, EqMode},
    services::autoeq::AutoEqEntry,
};

use crate::{
    Message, Nokkvi,
    update::modals::ActiveModal,
    widgets::{
        EqModalMessage,
        eq_autoeq::{
            AutoEqList, AutoEqMessage, AutoEqPanel, AutoEqStep, RESULTS_SCROLL_ID, SEARCH_INPUT_ID,
            row_id,
        },
    },
};

/// The version the requests' user agent names (`nokkvi/<version>`).
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn autoeq_message(msg: AutoEqMessage) -> Message {
    Message::EqModal(EqModalMessage::AutoEq(msg))
}

impl Nokkvi {
    /// The panel's own key, while the EQ modal is on top with the panel
    /// open: bare Up / Down move the highlight (read ahead of the text-field
    /// guard in `handle_raw_key_event`, since the focused search field
    /// captures the arrows), and Enter with no field focused loads the
    /// highlighted row (a focused field's Enter arrives as its `on_submit`).
    pub(crate) fn eq_autoeq_key(
        &self,
        key: &Key,
        modifiers: Modifiers,
        status: Status,
    ) -> Option<AutoEqMessage> {
        if self.eq_modal.autoeq.is_none()
            || !self.modal_is_top(ActiveModal::Eq)
            || self.playback.eq_state.mode() != EqMode::Parametric
            || modifiers.control()
            || modifiers.alt()
            || modifiers.logo()
            || modifiers.shift()
        {
            return None;
        }
        match key {
            Key::Named(Named::ArrowUp) => Some(AutoEqMessage::Step(AutoEqStep::Up)),
            Key::Named(Named::ArrowDown) => Some(AutoEqMessage::Step(AutoEqStep::Down)),
            Key::Named(Named::Enter) if status == Status::Ignored => Some(AutoEqMessage::Submit),
            Key::Named(_) | Key::Character(_) | Key::Unidentified => None,
        }
    }

    pub(crate) fn handle_eq_autoeq(&mut self, msg: AutoEqMessage) -> Task<Message> {
        match msg {
            AutoEqMessage::Open => self.open_eq_autoeq(),
            AutoEqMessage::Back => {
                self.eq_modal.autoeq = None;
                Task::none()
            }
            AutoEqMessage::SearchChanged(query) => {
                let Some(panel) = self.eq_modal.autoeq.as_mut() else {
                    return Task::none();
                };
                panel.query = query;
                panel.refilter();
                // The best match is highlighted: start the list at the top.
                iced::widget::operation::scrollable::snap_to(
                    RESULTS_SCROLL_ID,
                    iced::widget::scrollable::RelativeOffset::START,
                    iced::advanced::widget::operation::Animation::Instant,
                )
            }
            AutoEqMessage::Step(step) => {
                let Some(position) = self
                    .eq_modal
                    .autoeq
                    .as_mut()
                    .and_then(|panel| panel.step(step))
                else {
                    return Task::none();
                };
                crate::widgets::scroll_into_view::center_in_scrollable(
                    RESULTS_SCROLL_ID,
                    row_id(position),
                )
            }
            AutoEqMessage::Submit => {
                match self
                    .eq_modal
                    .autoeq
                    .as_ref()
                    .and_then(AutoEqPanel::highlighted_entry)
                {
                    Some(index) => self.choose_eq_autoeq(index),
                    None => Task::none(),
                }
            }
            AutoEqMessage::Choose(index) => self.choose_eq_autoeq(index),
            AutoEqMessage::Retry => {
                let refresh = match self.eq_modal.autoeq.as_ref().map(|panel| &panel.list) {
                    Some(AutoEqList::Failed { refresh, .. }) => *refresh,
                    Some(AutoEqList::Loading { .. } | AutoEqList::Ready(_)) | None => false,
                };
                self.request_eq_autoeq_index(refresh)
            }
            AutoEqMessage::Refresh => self.request_eq_autoeq_index(true),
            AutoEqMessage::IndexLoaded { generation, result } => {
                if !self.eq_modal.autoeq_generation.accepts(generation) {
                    return Task::none();
                }
                let Some(panel) = self.eq_modal.autoeq.as_mut() else {
                    return Task::none();
                };
                match result {
                    Ok(list) => panel.show(list),
                    Err(why) => {
                        let refresh = matches!(panel.list, AutoEqList::Loading { refresh: true });
                        panel.list = AutoEqList::Failed { why, refresh };
                        panel.refilter();
                    }
                }
                Task::none()
            }
            AutoEqMessage::ProfileLoaded {
                generation,
                entry,
                result,
            } => self.handle_eq_autoeq_profile(generation, &entry, result),
        }
    }

    /// AutoEq…: the panel, its search field focused. The list shows at once
    /// when this session already read it; else it is asked for (the open is
    /// the user's request).
    fn open_eq_autoeq(&mut self) -> Task<Message> {
        if self.playback.eq_state.mode() != EqMode::Parametric {
            return Task::none();
        }
        // The press took focus from a typed field: commit it first.
        let commit = self.commit_eq_field();
        let mut panel = AutoEqPanel::default();
        let session_list = self
            .app_service
            .as_ref()
            .and_then(nokkvi_data::backend::app_service::AppService::autoeq_session_index);
        let load = match session_list {
            Some(list) => {
                // Nothing in flight may land in this panel.
                self.eq_modal.autoeq_generation.bump();
                panel.show(list);
                self.eq_modal.autoeq = Some(panel);
                Task::none()
            }
            None => {
                self.eq_modal.autoeq = Some(panel);
                self.request_eq_autoeq_index(false)
            }
        };
        Task::batch([
            commit,
            load,
            crate::widgets::focus_text_input(SEARCH_INPUT_ID),
        ])
    }

    /// Ask for the list (`refresh`: download it again); the panel shows
    /// `Loading…` until it lands.
    fn request_eq_autoeq_index(&mut self, refresh: bool) -> Task<Message> {
        let Some(panel) = self.eq_modal.autoeq.as_mut() else {
            return Task::none();
        };
        panel.list = AutoEqList::Loading { refresh };
        panel.downloading = None;
        panel.refilter();
        let generation = self.eq_modal.autoeq_generation.bump();
        self.shell_task(
            move |shell| async move { shell.autoeq_index(refresh, APP_VERSION).await },
            move |result| {
                autoeq_message(AutoEqMessage::IndexLoaded {
                    generation,
                    result: result.map_err(|e| format!("{e:#}")),
                })
            },
        )
    }

    /// Download entry `index`'s profile (the row is highlighted and says
    /// `Loading…` meanwhile). A second press on the row already loading asks
    /// nothing; a press on another row supersedes it.
    fn choose_eq_autoeq(&mut self, index: usize) -> Task<Message> {
        let Some(panel) = self.eq_modal.autoeq.as_mut() else {
            return Task::none();
        };
        if panel.downloading == Some(index) {
            return Task::none();
        }
        let Some(entry) = panel.entry(index).cloned() else {
            return Task::none();
        };
        panel.downloading = Some(index);
        if let Some(position) = panel.matches.shown.iter().position(|&shown| shown == index) {
            panel.highlight = Some(position);
        }
        let generation = self.eq_modal.autoeq_generation.bump();
        let asked = entry.clone();
        self.shell_task(
            move |shell| async move { shell.autoeq_profile(&asked, APP_VERSION).await },
            move |result| {
                autoeq_message(AutoEqMessage::ProfileLoaded {
                    generation,
                    entry,
                    result: result.map_err(|e| format!("{e:#}")),
                })
            },
        )
    }

    /// A downloaded profile: back to the editor with it in place, as an
    /// import puts a file in; a failure toasts why and keeps the panel.
    fn handle_eq_autoeq_profile(
        &mut self,
        generation: u64,
        entry: &AutoEqEntry,
        result: Result<Vec<EqBand>, String>,
    ) -> Task<Message> {
        if !self.eq_modal.autoeq_generation.accepts(generation) || self.eq_modal.autoeq.is_none() {
            return Task::none();
        }
        match result {
            Ok(bands) => {
                self.eq_modal.autoeq = None;
                let loaded = format!("Loaded {} ({})", entry.name, entry.source);
                self.load_eq_profile(
                    &bands,
                    entry.name.clone(),
                    "save the AutoEq profile",
                    move |_| loaded,
                )
            }
            Err(why) => {
                if let Some(panel) = self.eq_modal.autoeq.as_mut() {
                    panel.downloading = None;
                }
                self.toast_error(format!("Couldn't load {}: {why}", entry.name));
                Task::none()
            }
        }
    }
}
