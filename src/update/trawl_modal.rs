//! Trawl mix-builder modal handler.
//!
//! Mirrors the two proven shapes it sits between: the default-playlist
//! picker's open/close/focus lifecycle and Harbour's whole-library search
//! (immediate fire, [`SEARCH_MIN_CHARS`] gate, root-owned generation
//! stale-drop — `trawl_search_generation` lives on `Nokkvi`, not the modal
//! state, so close/reopen can never re-mint a generation an in-flight
//! fan-out already captured). Play/enqueue route through
//! `AppService::{play_trawl, add_trawl_to_queue}` with the same guard +
//! session-expiry conventions as `play_batch_task`; errors keep the modal
//! open so "Mix is empty — every song was under 1:00" lands where the user
//! can act on it.

use iced::Task;
use tracing::error;

use crate::{
    Nokkvi,
    app_message::Message,
    views::harbour::{SEARCH_MIN_CHARS, SEARCH_PREVIEW_LIMIT},
    widgets::trawl_modal::{
        TRAWL_SEARCH_INPUT_ID, TrawlModalMessage, TrawlModalState, TrawlRow, TrawlTrayControl,
        build_trawl_rows, cycle_tray_cursor,
    },
};

impl Nokkvi {
    pub(crate) fn handle_trawl_modal(&mut self, msg: TrawlModalMessage) -> Task<Message> {
        match msg {
            TrawlModalMessage::Open => {
                // The mix builder never opens over Settings — the global `t`
                // hotkey is view-agnostic, so the gate lives here (the Harbour
                // door and Queue header can't fire from Settings anyway).
                if self.current_view == crate::View::Settings {
                    return Task::none();
                }
                // Bump the generation so an in-flight fan-out from BEFORE a
                // close can't land in this fresh modal (Close doesn't bump —
                // the reopened state must not accept the old query's result).
                self.trawl_search_generation = self.trawl_search_generation.wrapping_add(1);
                self.trawl_modal = Some(TrawlModalState {
                    search_input_focused: true,
                    ..TrawlModalState::default()
                });
                iced::widget::operation::focus(TRAWL_SEARCH_INPUT_ID)
            }
            TrawlModalMessage::Close => {
                self.trawl_modal = None;
                Task::none()
            }
            TrawlModalMessage::SearchChanged(query) => self.handle_trawl_search(query),
            TrawlModalMessage::SearchLoaded { generation, result } => {
                if generation != self.trawl_search_generation {
                    return Task::none();
                }
                // A 401 routes to session expiry even if the modal was closed
                // while the fan-out was in flight — the session is dead either
                // way and swallowing it would strand the user until the next
                // server call.
                if let Err(e) = &result
                    && nokkvi_data::types::error::NokkviError::is_unauthorized_str(e)
                {
                    return self.handle_session_expired();
                }
                let Some(state) = self.trawl_modal.as_mut() else {
                    return Task::none();
                };
                state.search_loading = false;
                match result {
                    Ok(results) => {
                        state.search_results = Some(*results);
                        self.warm_trawl_search_artwork()
                    }
                    Err(e) => {
                        // Drop the previous query's results too — leaving them
                        // would keep rendering rows that no longer match what
                        // the user typed (the failed-search hint shows instead).
                        state.search_results = None;
                        self.toast_error(format!("Search failed: {e}"));
                        Task::none()
                    }
                }
            }
            TrawlModalMessage::SlotListUp => {
                let total = self.trawl_row_count();
                if let Some(state) = self.trawl_modal.as_mut() {
                    state.slot_list.move_up(total);
                }
                Task::none()
            }
            TrawlModalMessage::SlotListDown => {
                let total = self.trawl_row_count();
                if let Some(state) = self.trawl_modal.as_mut() {
                    state.slot_list.move_down(total);
                }
                Task::none()
            }
            TrawlModalMessage::SlotListSetOffset(offset) => {
                let total = self.trawl_row_count();
                if let Some(state) = self.trawl_modal.as_mut() {
                    state.slot_list.set_offset(offset, total);
                }
                Task::none()
            }
            TrawlModalMessage::ClickRow(index) => {
                self.toggle_trawl_row(index);
                Task::none()
            }
            TrawlModalMessage::ActivateCenter => {
                let center = self.trawl_modal.as_ref().and_then(|state| {
                    state
                        .slot_list
                        .get_effective_center_index(self.trawl_row_count_for(state))
                });
                if let Some(index) = center {
                    self.toggle_trawl_row(index);
                }
                Task::none()
            }
            TrawlModalMessage::RemoveSeed(index) => {
                self.trawl_crate.remove_at(index);
                Task::none()
            }
            TrawlModalMessage::IncWeight(index) => {
                if let Some(seed) = self.trawl_crate.seeds.get_mut(index) {
                    seed.weight =
                        (seed.weight + 1).min(nokkvi_data::types::trawl::TRAWL_WEIGHT_MAX);
                }
                Task::none()
            }
            TrawlModalMessage::DecWeight(index) => {
                if let Some(seed) = self.trawl_crate.seeds.get_mut(index) {
                    seed.weight = seed
                        .weight
                        .saturating_sub(1)
                        .max(nokkvi_data::types::trawl::TRAWL_WEIGHT_MIN);
                }
                Task::none()
            }
            TrawlModalMessage::SetBlend(blend) => {
                self.trawl_crate.blend = blend;
                Task::none()
            }
            TrawlModalMessage::SetMinLength(min) => {
                self.trawl_crate.min_length = min;
                Task::none()
            }
            TrawlModalMessage::SetMaxLength(max) => {
                self.trawl_crate.max_length = max;
                Task::none()
            }
            TrawlModalMessage::SetRating(filter) => {
                self.trawl_crate.rating = filter;
                Task::none()
            }
            TrawlModalMessage::SetMaxTracks(max) => {
                self.trawl_crate.max_tracks = max;
                Task::none()
            }
            TrawlModalMessage::ClearCrate => {
                self.trawl_crate.clear_seeds();
                Task::none()
            }
            TrawlModalMessage::PlayMix => {
                if self.trawl_crate.is_empty() {
                    // Reachable via Ctrl+Enter with nothing seeded — say why
                    // nothing happened instead of silently ignoring the press.
                    self.toast_warn("The crate is empty — add seeds first");
                    return Task::none();
                }
                // Same pre-play ritual as play_batch_task's callers: radio →
                // queue transition, then reset the previous playback context
                // (loading target + active playlist).
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                let mix = self.trawl_crate.clone();
                self.shell_task(
                    move |shell| async move { shell.play_trawl(&mix).await },
                    |result| {
                        Message::TrawlModal(TrawlModalMessage::PlayMixCompleted(
                            result.map_err(|e| format!("{e:#}")),
                        ))
                    },
                )
            }
            TrawlModalMessage::PlayMixCompleted(result) => match result {
                Ok(()) => {
                    self.trawl_modal = None;
                    Task::done(Message::Navigation(
                        crate::app_message::NavigationMessage::SwitchView(crate::View::Queue),
                    ))
                }
                Err(e) => {
                    if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&e) {
                        return self.handle_session_expired();
                    }
                    error!(" Failed to play mix: {e}");
                    // Keep the modal open — "every song was under 1:00" is
                    // actionable right here (lower the minimum, re-play).
                    self.toast_error(format!("Failed to play mix: {e}"));
                    Task::none()
                }
            },
            TrawlModalMessage::AddMixToQueue => {
                if self.trawl_crate.is_empty() {
                    // Reachable via Shift+A with nothing seeded — same warn as
                    // PlayMix so both keyboard CTAs explain the no-op.
                    self.toast_warn("The crate is empty — add seeds first");
                    return Task::none();
                }
                let mix = self.trawl_crate.clone();
                self.shell_task(
                    move |shell| async move { shell.add_trawl_to_queue(&mix).await },
                    |result| {
                        Message::TrawlModal(TrawlModalMessage::AddMixCompleted(
                            result.map_err(|e| format!("{e:#}")),
                        ))
                    },
                )
            }
            TrawlModalMessage::AddMixCompleted(result) => match result {
                Ok(count) => {
                    let noun = if count == 1 { "song" } else { "songs" };
                    self.toast_success(format!("Added {count} {noun} to queue"));
                    Task::none()
                }
                Err(e) => {
                    if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&e) {
                        return self.handle_session_expired();
                    }
                    error!(" Failed to add mix to queue: {e}");
                    self.toast_error(format!("Failed to add mix to queue: {e}"));
                    Task::none()
                }
            },
            TrawlModalMessage::ChipsScrolled(delta) => iced::advanced::widget::operate(
                iced::advanced::widget::operation::scrollable::scroll_by(
                    crate::widgets::trawl_modal::chips_scrollable_id(),
                    iced::widget::scrollable::AbsoluteOffset { x: delta, y: 0.0 },
                ),
            ),
        }
    }

    /// Immediate search with the shared min-chars gate and per-keystroke
    /// generation bump (Harbour's stale-drop shape, root-owned counter).
    fn handle_trawl_search(&mut self, query: String) -> Task<Message> {
        self.trawl_search_generation = self.trawl_search_generation.wrapping_add(1);
        let generation = self.trawl_search_generation;

        let Some(state) = self.trawl_modal.as_mut() else {
            return Task::none();
        };
        state.search_query = query;
        // Typing proves the field holds focus (blur events don't reach us).
        state.search_input_focused = true;
        // ...and a focused field owns the arrow keys, so the tray focus ring
        // clears — it must never show while Left/Right move the text caret.
        state.tray_cursor = None;
        // Fresh query, fresh viewport — a deep scroll into the previous
        // results must not strand the center past a shorter list.
        state.slot_list = crate::widgets::slot_list_view::SlotListView::new();

        let trimmed = state.search_query.trim().to_string();
        if trimmed.chars().count() < SEARCH_MIN_CHARS {
            state.search_results = None;
            state.search_loading = false;
            return Task::none();
        }

        state.search_loading = true;
        self.shell_task(
            move |shell| async move {
                let ids = shell.active_library_ids_vec();
                shell
                    .search_library(&trimmed, SEARCH_PREVIEW_LIMIT, &ids)
                    .await
            },
            move |result| {
                Message::TrawlModal(TrawlModalMessage::SearchLoaded {
                    generation,
                    result: result.map(Box::new).map_err(|e| format!("{e:#}")),
                })
            },
        )
    }

    /// Shift+Tab / Shift+Backspace with the modal open: one step around the
    /// tray-controls focus ring (`None → Blend → … → MaxTracks → None`).
    ///
    /// Forward motion always unfocuses the search field: the flag is a
    /// best-effort mirror of iced focus (a mouse click into the field doesn't
    /// set it until the next keystroke), so the sentinel focus op is issued
    /// unconditionally — harmless when nothing is focused, and it guarantees
    /// Left/Right go live the instant the ring appears. Backward motion never
    /// unfocuses, and a backward press that a truly focused search field
    /// consumed as a character deletion is swallowed upstream at the raw-key
    /// gate (status-keyed — see `handle_raw_key_event`): by the time this
    /// runs, a backward step is always intentional.
    pub(crate) fn handle_trawl_tray_focus_move(&mut self, forward: bool) -> Task<Message> {
        let Some(state) = self.trawl_modal.as_mut() else {
            return Task::none();
        };
        state.tray_cursor = cycle_tray_cursor(state.tray_cursor, forward);
        if forward {
            state.search_input_focused = false;
            return super::components::unfocus_all();
        }
        Task::none()
    }

    /// Left/Right with the modal open: cycle the ring-focused tray control's
    /// value through its const `ALL` array, wrapping both directions (the
    /// settings Enum precedent). No focused control → no-op: the ring's
    /// `None` position is a deliberate rest state, not an implicit re-entry.
    ///
    /// Delegates to the existing `Set*` arms so keyboard and mouse share ONE
    /// crate-write path. Known accepted quirk: a mouse-opened pick_list
    /// dropdown renders a stale menu while a value cycles underneath — iced
    /// exposes no way to read or close the menu from the app, the crate value
    /// stays authoritative, and the menu self-heals on close.
    pub(crate) fn handle_trawl_tray_cycle_value(&mut self, forward: bool) -> Task<Message> {
        use nokkvi_data::{
            types::trawl::{
                TrawlBlend, TrawlMaxLength, TrawlMaxTracks, TrawlMinLength, TrawlRatingFilter,
            },
            utils::cycle::cycle_wrapping,
        };

        let Some(control) = self.trawl_modal.as_ref().and_then(|s| s.tray_cursor) else {
            return Task::none();
        };
        let msg = match control {
            TrawlTrayControl::Blend => TrawlModalMessage::SetBlend(cycle_wrapping(
                &TrawlBlend::ALL,
                self.trawl_crate.blend,
                forward,
            )),
            TrawlTrayControl::MinLength => TrawlModalMessage::SetMinLength(cycle_wrapping(
                &TrawlMinLength::ALL,
                self.trawl_crate.min_length,
                forward,
            )),
            TrawlTrayControl::MaxLength => TrawlModalMessage::SetMaxLength(cycle_wrapping(
                &TrawlMaxLength::ALL,
                self.trawl_crate.max_length,
                forward,
            )),
            TrawlTrayControl::Rating => TrawlModalMessage::SetRating(cycle_wrapping(
                &TrawlRatingFilter::ALL,
                self.trawl_crate.rating,
                forward,
            )),
            TrawlTrayControl::MaxTracks => TrawlModalMessage::SetMaxTracks(cycle_wrapping(
                &TrawlMaxTracks::ALL,
                self.trawl_crate.max_tracks,
                forward,
            )),
        };
        self.handle_trawl_modal(msg)
    }

    /// Toggle the seed carried by row `index` in/out of the crate. Headers,
    /// hints, and out-of-range indices no-op.
    fn toggle_trawl_row(&mut self, index: usize) {
        let Some(state) = self.trawl_modal.as_ref() else {
            return;
        };
        let rows = build_trawl_rows(state, &self.trawl_crate);
        if let Some(TrawlRow::Result { seed, .. }) = rows.into_iter().nth(index) {
            self.trawl_crate.toggle(seed);
        }
    }

    /// Row count through the single row-order source (render parity).
    fn trawl_row_count(&self) -> usize {
        self.trawl_modal
            .as_ref()
            .map_or(0, |state| self.trawl_row_count_for(state))
    }

    fn trawl_row_count_for(&self, state: &TrawlModalState) -> usize {
        build_trawl_rows(state, &self.trawl_crate).len()
    }

    /// Warm 80px minis for the modal's search results: album covers for
    /// album/song rows, artist images for artist rows. Reuses Harbour's warm
    /// plumbing (shared caches, shared dedupe). Genres/playlists render a
    /// type glyph in the modal — no quad fan-out here.
    fn warm_trawl_search_artwork(&mut self) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();

        let mut id_slices: Vec<Vec<String>> = Vec::new();
        let mut artist_ids: Vec<String> = Vec::new();
        if let Some(state) = &self.trawl_modal
            && let Some(r) = &state.search_results
        {
            for a in &r.albums {
                id_slices.push(vec![a.id.clone()]);
            }
            for s in &r.songs {
                if let Some(id) = &s.album_id {
                    id_slices.push(vec![id.clone()]);
                }
            }
            artist_ids = r.artists.iter().map(|a| a.id.clone()).collect();
        }

        let mut tasks: Vec<Task<Message>> = Vec::new();
        if !id_slices.is_empty() {
            tasks.extend(self.warm_harbour_quad_ids(&albums_vm, id_slices));
        }
        tasks.extend(self.artist_mini_warm_tasks(artist_ids, &albums_vm));
        Task::batch(tasks)
    }
}
