//! Rules-editor session handlers — `EditorSessionKind::Rules` inside
//! `View::PlaylistEditor`.
//!
//! M4 scope: open (one-screen create / edit), author with full validation
//! and version gating, save DIRECT to the target (concurrency-guarded,
//! target-404-aware), and the container ruling's honest post-save observe
//! loop. The M5 draft-preview engine replaces the interim controls
//! (create sessions render NO Preview control here; edit sessions get the
//! read-only "Re-evaluate" of the SAVED rules).

use iced::Task;
use nokkvi_data::types::{
    playlist::Playlist,
    rules_session::RulesTarget,
    smart_criteria::{
        Conjunction, CriteriaNode, FieldClass, FieldKind, RuleOperator, SEED_PRESETS, SmartRules,
        ValueShape,
    },
};
use tracing::{debug, info, warn};

use crate::{
    Nokkvi, View,
    app_message::{Message, RulesEditorMessage, RulesEntryTarget},
    state::{
        EditingCell, FormCell, FormMode, FormRow, JsonModeState, PreviewPhase, RulesPane,
        RulesSessionUi, SubPicker, SubPickerKind,
    },
    views,
};

/// The evaluation read's payload: (rows, X-Total-Count, evaluatedAt).
type EvalPayload = (
    Vec<nokkvi_data::types::song::Song>,
    Option<u32>,
    Option<String>,
);

/// A draft write + read's payload: (draft_id, rows, total, evaluatedAt).
type DraftEvalPayload = (
    String,
    Vec<nokkvi_data::types::song::Song>,
    Option<u32>,
    Option<String>,
);

/// Mint the strict-grammar draft marker for THIS write (fresh `ts` every
/// time — an actively-previewing session never ages out of the sweep's
/// protection).
fn mint_draft_marker() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    nokkvi_data::types::playlist::DraftMarker::format(1, std::process::id(), ts)
}

/// How many rows the interim evaluation reads (mirrors the ruled preview
/// page size).
const PREVIEW_PAGE: u32 = 30;

/// Post-save observe loop: bounded re-polls while the stamp hasn't
/// advanced (≤3 over ~12 s).
const OBSERVE_RETRIES: u8 = 3;
const OBSERVE_RETRY_SECS: u64 = 4;

impl Nokkvi {
    // =====================================================================
    // Session open (the ONE seeding point — every entry funnel lands here)
    // =====================================================================

    pub(crate) fn handle_enter_rules_mode(&mut self, target: RulesEntryTarget) -> Task<Message> {
        // Refuse to clobber an open editor — mounting a fresh session over a
        // live one silently discards its edits AND orphans its draft
        // playlist (the mount replaces `playlist_editor` outright). Mirrors
        // the create-dialog guard. Reachable e.g. via Shift+N while already
        // in a rules session.
        if self.playlist_editor.is_some() {
            self.toast_warn("Finish or discard the current playlist edit first");
            return Task::none();
        }
        if !self.caps_state.smart_available() {
            // Defensive backstop — the entry points are caps-gated; reaching
            // here means a stale surface or a rebind race.
            self.toast_warn(
                "Smart playlists need Navidrome 0.61+ (or the server version is unknown)",
            );
            return Task::none();
        }

        let caps = self.caps_state.caps();
        let (edit_state, session) = match &target {
            RulesEntryTarget::Create => {
                // One-screen create: placeholder name, private, focus seeded
                // in Editing mode on the name input. ZERO draft network at
                // open (the M5 blank-create rule); the ONLY dispatch below
                // is the session-open playlists-list read + tag discovery.
                let edit = nokkvi_data::types::playlist_edit::PlaylistEditState::new(
                    String::new(),
                    String::new(),
                    String::new(),
                    false,
                    Vec::new(),
                );
                let session =
                    RulesSessionUi::open(RulesTarget::Create, SmartRules::new_empty(), caps);
                (edit, session)
            }
            RulesEntryTarget::Edit { playlist_id } => {
                let Some(row) = self
                    .library
                    .playlists
                    .iter()
                    .find(|p| p.id == *playlist_id)
                    .cloned()
                else {
                    self.toast_warn("Open it from the Playlists view to edit");
                    return Task::none();
                };
                if !views::playlists::view::playlist_is_owned(&row.owner_id, &self.session_user_id)
                {
                    self.toast_warn("Only the playlist's owner can edit its rules");
                    return Task::none();
                }
                // The raw rules substrate rides the projection.
                let Some(raw_rules) = row.rules.clone() else {
                    // The row wasn't smart (or the substrate is absent) —
                    // the entry points shouldn't have offered Edit Rules.
                    self.toast_warn("Couldn't load this playlist's rules — refresh and retry");
                    return Task::none();
                };
                let edit = nokkvi_data::types::playlist_edit::PlaylistEditState::new(
                    row.id.clone(),
                    row.name.clone(),
                    row.comment.clone(),
                    row.public,
                    Vec::new(),
                );
                let rules = SmartRules::parse(&raw_rules);
                let mut session = RulesSessionUi::open(
                    RulesTarget::Edit {
                        playlist_id: row.id.clone(),
                        file_backed: row.is_file_backed,
                        sync: row.sync,
                        loaded_updated_at: row.updated_at.clone(),
                    },
                    rules,
                    caps,
                );
                session.preview.evaluated_at = row.evaluated_at.clone();
                (edit, session)
            }
        };

        info!(" Entering rules session ({:?})", target);
        self.mount_rules_session(edit_state, session)
    }

    /// Enter an EDIT rules session straight from a fetched playlist record
    /// — the pencil's JIT lane, where the row may not be in the library
    /// list. Ownership/caps are re-checked here (single seeding point
    /// discipline).
    pub(crate) fn enter_rules_mode_from_meta(&mut self, meta: &Playlist) -> Task<Message> {
        if !self.caps_state.smart_available() {
            self.toast_warn(
                "Smart playlists need Navidrome 0.61+ (or the server version is unknown)",
            );
            return Task::none();
        }
        if !views::playlists::view::playlist_is_owned(&meta.owner_id, &self.session_user_id) {
            self.toast_warn("Only the playlist's owner can edit its rules");
            return Task::none();
        }
        let Some(raw_rules) = meta.rules.as_ref().filter(|r| !r.is_null()) else {
            self.toast_warn("This playlist has no rules to edit");
            return Task::none();
        };
        let edit_state = nokkvi_data::types::playlist_edit::PlaylistEditState::new(
            meta.id.clone(),
            meta.name.clone(),
            meta.comment.clone(),
            meta.public,
            Vec::new(),
        );
        let mut session = RulesSessionUi::open(
            RulesTarget::Edit {
                playlist_id: meta.id.clone(),
                file_backed: meta.is_file_backed(),
                sync: meta.sync,
                loaded_updated_at: meta.updated_at.clone(),
            },
            SmartRules::parse(raw_rules),
            self.caps_state.caps(),
        );
        session.preview.evaluated_at = meta.evaluated_at.clone();
        info!(" Entering rules session (JIT meta for {})", meta.id);
        self.mount_rules_session(edit_state, session)
    }

    /// The shared mounting tail: install the session, drop the browsing
    /// panel, navigate, and dispatch the session-open data fetches.
    fn mount_rules_session(
        &mut self,
        edit_state: nokkvi_data::types::playlist_edit::PlaylistEditState,
        session: RulesSessionUi,
    ) -> Task<Message> {
        let is_edit_target = matches!(session.target, RulesTarget::Edit { .. });
        // Warm the custom cover before `session` moves into the editor (the
        // quad's album covers ride the preview prefetch; a create session has
        // no playlist id yet).
        let cover_playlist_id = match &session.target {
            RulesTarget::Edit { playlist_id, .. } => Some(playlist_id.clone()),
            RulesTarget::Create => None,
        };
        let mut edit_state = edit_state;
        // Reuse the Tracks lane's optimistic-concurrency token slot.
        if let RulesTarget::Edit {
            loaded_updated_at, ..
        } = &session.target
            && !loaded_updated_at.is_empty()
        {
            edit_state.set_loaded_updated_at(loaded_updated_at.clone());
        }
        self.playlist_editor = Some(crate::state::PlaylistEditorState::new_rules(
            edit_state, session,
        ));
        // No browsing panel in rules mode — dead weight (the server refuses
        // track mutations anyway).
        self.browsing_panel = None;
        self.pane_focus = crate::state::PaneFocus::Queue;
        self.queue_page.playlist_strip_expanded = false;
        self.editor_return_view = if self.current_view == View::PlaylistEditor {
            View::Playlists
        } else {
            self.current_view
        };
        self.current_view = View::PlaylistEditor;

        // Session-open data dependencies: the playlists list (sub-picker
        // rows + list-gated diagnostics — the queue lanes prove
        // `library.playlists` can be unpopulated) and tag discovery.
        let list_fetch = self.shell_task(
            |shell| async move {
                let service = shell.playlists_api().await?;
                let library_ids = shell.active_library_ids_vec();
                let (playlists, _) = service
                    .load_playlists_with_libraries("name", "ASC", None, &library_ids)
                    .await?;
                Ok(playlists
                    .into_iter()
                    .map(|p| (p.id, p.name))
                    .collect::<Vec<_>>())
            },
            |result: Result<Vec<(String, String)>, anyhow::Error>| match result {
                Ok(list) => Message::RulesEditor(RulesEditorMessage::SessionPlaylistsLoaded(list)),
                Err(e) => {
                    warn!("rules session playlists fetch failed: {e:#}");
                    Message::RulesEditor(RulesEditorMessage::SessionPlaylistsLoaded(Vec::new()))
                }
            },
        );
        let tag_fetch = self.shell_task(
            |shell| async move {
                let tags = shell.tags_api().await?.list_all_tags().await?;
                Ok(tags
                    .into_iter()
                    .map(|t| (t.tag_name, t.tag_value, t.song_count))
                    .collect::<Vec<_>>())
            },
            |result: Result<Vec<(String, String, u64)>, anyhow::Error>| match result {
                Ok(rows) => Message::RulesEditor(RulesEditorMessage::TagsDiscovered(Ok(rows))),
                Err(e) => {
                    Message::RulesEditor(RulesEditorMessage::TagsDiscovered(Err(format!("{e:#}"))))
                }
            },
        );
        // Edit sessions (and preset-seeded creates via PresetChosen) have
        // real rules at open: POST the draft immediately — builder-open IS
        // the first preview, always fresh on every released server (the
        // create-path EvaluatedAt nil). Blank creates dispatch NOTHING
        // draft-related here (the ruled zero-network-at-open pin).
        let initial_eval = if is_edit_target {
            self.dispatch_draft_preview()
        } else {
            Task::none()
        };
        // A create session opens in Editing mode on the name field — focus
        // it so the user can type immediately (edit sessions open in Cursor
        // mode and need no input focus).
        let initial_focus = self.rules_focus_task();
        let cover_fetch = cover_playlist_id.map_or_else(Task::none, |id| {
            self.fetch_playlist_custom_mini_task(id, None)
        });
        Task::batch([
            list_fetch,
            tag_fetch,
            initial_eval,
            initial_focus,
            cover_fetch,
        ])
    }

    // =====================================================================
    // Message dispatch
    // =====================================================================

    pub(crate) fn handle_rules_editor(&mut self, msg: RulesEditorMessage) -> Task<Message> {
        match msg {
            // --- cursor grammar ------------------------------------------
            RulesEditorMessage::CursorMove { down } => {
                let in_results = self
                    .rules_session()
                    .is_some_and(|s| s.pane == RulesPane::Results);
                self.with_rules_session(|s| {
                    if s.pane == RulesPane::Results {
                        let len = s.preview.rows.len();
                        if len > 0 {
                            if down {
                                s.preview.cursor = (s.preview.cursor + 1).min(len - 1);
                            } else {
                                s.preview.cursor = s.preview.cursor.saturating_sub(1);
                            }
                        }
                    } else if s.mode == FormMode::Cursor {
                        s.move_cursor(down);
                    }
                });
                // In the results pane, keep the moved cursor centered in view
                // (the pane is a plain scrollable — it doesn't follow on its
                // own). Measured scroll, same helper the settings pane uses.
                let follow = if in_results {
                    crate::widgets::scroll_into_view::center_in_scrollable(
                        iced::widget::Id::new(
                            crate::views::playlist_editor::rules_view::RULES_PREVIEW_SCROLLABLE_ID,
                        ),
                        iced::widget::Id::new(
                            crate::views::playlist_editor::rules_view::RULES_PREVIEW_CURSOR_ID,
                        ),
                    )
                } else {
                    Task::none()
                };
                // Crossing the loaded boundary pages the SAME completed
                // evaluation (`_start > 0` never re-writes — the `_start==0`
                // gate holds by construction).
                if down {
                    return Task::batch([self.maybe_fetch_preview_page(), follow]);
                }
                Task::batch([follow])
            }
            RulesEditorMessage::StepCell => {
                self.with_rules_session(|s| {
                    if s.pane == RulesPane::Form && s.mode == FormMode::Cursor {
                        s.step_cell();
                    }
                });
                Task::none()
            }
            RulesEditorMessage::CycleCell { forward } => self.handle_cycle_cell(forward),
            RulesEditorMessage::EnterOnCursor => self.handle_enter_on_cursor(),
            RulesEditorMessage::DeleteCursorRow => {
                self.with_rules_session_revalidate(|s| {
                    if s.mode != FormMode::Cursor || s.pane != RulesPane::Form {
                        return;
                    }
                    match s.rows.get(s.cursor).cloned() {
                        Some(FormRow::Rule(path) | FormRow::GroupHeader(path))
                            if s.form_editable() =>
                        {
                            s.remove_node(&path);
                        }
                        Some(FormRow::SortKey(i)) => s.remove_sort_key(i),
                        _ => {}
                    }
                });
                Task::none()
            }
            RulesEditorMessage::MoveCursorRow { up } => {
                self.with_rules_session_revalidate(|s| {
                    if let Some(FormRow::SortKey(i)) = s.rows.get(s.cursor).cloned() {
                        s.move_sort_key(i, up);
                    }
                });
                Task::none()
            }
            RulesEditorMessage::SwitchPane => {
                self.with_rules_session(|s| {
                    s.pane = match s.pane {
                        RulesPane::Form => RulesPane::Results,
                        RulesPane::Results => RulesPane::Form,
                    };
                });
                Task::none()
            }
            RulesEditorMessage::EscapePressed => self.handle_rules_escape(),
            RulesEditorMessage::ConfirmDiscard => {
                // While a save is in flight, deleting the draft would race the
                // promote-PUT (a create-save finalizes the draft INTO the real
                // playlist) and destroy the just-saved playlist. Keep the
                // confirm up; the save clears `saving` in a moment.
                if self.rules_session().is_some_and(|s| s.saving) {
                    self.toast_warn("Saving… — try discarding again in a moment");
                    return Task::none();
                }
                info!(" Rules session discarded");
                let cleanup = self
                    .rules_session()
                    .and_then(|s| s.draft.as_ref().map(|d| d.id.clone()))
                    .map_or_else(Task::none, |id| self.delete_draft_task(id));
                Task::batch([cleanup, self.handle_exit_playlist_edit_mode()])
            }
            RulesEditorMessage::CancelDiscard => {
                self.with_rules_session(|s| s.confirm_discard = false);
                Task::none()
            }
            RulesEditorMessage::ClickCell { row, cell } => {
                // Commit any in-progress edit FIRST. Otherwise the click lands
                // with mode still Editing, so handle_enter_on_cursor no-ops
                // (the "click does nothing while a field is active" bug) and
                // the stale editing buffer bleeds into whatever Value cell is
                // clicked next (the "genre shows 2 with a caret" bug).
                if self
                    .rules_session()
                    .is_some_and(|s| s.mode == FormMode::Editing)
                {
                    self.commit_editing_cell();
                    self.revalidate_rules_session();
                }
                self.with_rules_session(|s| {
                    if row < s.rows.len() {
                        s.pane = RulesPane::Form;
                        s.cursor = row;
                        s.cell = cell;
                        s.normalize_cell();
                    }
                });
                // One write path: a click performs the same per-cell action
                // as Enter.
                self.handle_enter_on_cursor()
            }

            // --- editing mode --------------------------------------------
            RulesEditorMessage::EditingInput(text) => {
                self.with_rules_session(|s| {
                    if let Some(editing) = s.editing.as_mut() {
                        editing.buffer = text;
                    }
                });
                Task::none()
            }
            RulesEditorMessage::CommitEditing => {
                self.commit_editing_cell();
                self.revalidate_rules_session();
                // Blur back to a focus-free Cursor mode. The always-rendered
                // edit-bar name/comment inputs would otherwise keep focus and
                // capture the next Left/Right (re-entering Editing); value
                // cells unmount on commit, so the blur is a no-op for them.
                crate::update::components::unfocus_all()
            }
            RulesEditorMessage::RevertEditing => {
                // Escape in Editing: revert the CELL, defocus only — the
                // session survives (one press, one meaning). Value/limit cells
                // hold their edit in the buffer, so dropping it IS the revert.
                // The edit-bar Name/Comment inputs live-commit each keystroke
                // to editor.edit, so reverting the cell means restoring the
                // snapshot captured on entry through the same write path —
                // otherwise Escape leaves the typed change (the grammar lie).
                let restore = self.rules_session().and_then(|s| {
                    s.editing.as_ref().and_then(|e| match (&e.row, e.cell) {
                        (FormRow::EditBar, FormCell::Name) => Some(
                            crate::app_message::EditorMessage::NameChanged(e.revert.clone()),
                        ),
                        (FormRow::EditBar, FormCell::Comment) => Some(
                            crate::app_message::EditorMessage::CommentChanged(e.revert.clone()),
                        ),
                        _ => None,
                    })
                });
                self.with_rules_session(|s| {
                    s.editing = None;
                    s.mode = FormMode::Cursor;
                });
                let restore_task =
                    restore.map_or_else(Task::none, |m| self.update(Message::Editor(m)));
                Task::batch([restore_task, crate::update::components::unfocus_all()])
            }

            // --- sub-pickers ---------------------------------------------
            RulesEditorMessage::SubPickerQuery(query) => {
                self.with_rules_session(|s| {
                    if let Some(picker) = s.sub_picker.as_mut() {
                        picker.query = query;
                        picker.cursor = 0;
                    }
                });
                Task::none()
            }
            RulesEditorMessage::SubPickerMove { down } => {
                // Clamp to the rendered window: entries past the cap aren't
                // drawn, so the keyboard cursor must not walk onto one (Enter
                // would commit an unrendered row).
                let entry_count = self
                    .rules_session()
                    .and_then(|s| {
                        s.sub_picker
                            .as_ref()
                            .map(|p| rules_picker_entries(s, p).len())
                    })
                    .unwrap_or(0)
                    .min(crate::views::playlist_editor::rules_view::RULES_PICKER_RENDER_CAP);
                self.with_rules_session(|s| {
                    if let Some(picker) = s.sub_picker.as_mut()
                        && entry_count > 0
                    {
                        if down {
                            picker.cursor = (picker.cursor + 1).min(entry_count - 1);
                        } else {
                            picker.cursor = picker.cursor.saturating_sub(1);
                        }
                    }
                });
                // The picker list is a plain scrollable — follow the cursor so
                // the selected entry stays visible as it walks a long list.
                crate::widgets::scroll_into_view::center_in_scrollable(
                    iced::widget::Id::new(
                        crate::views::playlist_editor::rules_view::RULES_SUB_PICKER_SCROLLABLE_ID,
                    ),
                    iced::widget::Id::new(
                        crate::views::playlist_editor::rules_view::RULES_SUB_PICKER_CURSOR_ID,
                    ),
                )
            }
            RulesEditorMessage::SubPickerCommit => {
                self.commit_sub_picker();
                self.revalidate_rules_session();
                Task::none()
            }
            RulesEditorMessage::SubPickerCancel => {
                self.with_rules_session(|s| s.sub_picker = None);
                Task::none()
            }
            RulesEditorMessage::ClickPickerRow(index) => {
                self.with_rules_session(|s| {
                    if let Some(picker) = s.sub_picker.as_mut() {
                        picker.cursor = index;
                    }
                });
                self.commit_sub_picker();
                self.revalidate_rules_session();
                Task::none()
            }

            // --- date picker ---------------------------------------------
            RulesEditorMessage::DatePickerShiftMonth { forward } => {
                self.with_rules_session(|s| {
                    if let Some(SubPicker {
                        kind: SubPickerKind::DateValue { year, month, .. },
                        cursor,
                        ..
                    }) = s.sub_picker.as_mut()
                    {
                        let (ny, nm) =
                            nokkvi_data::utils::calendar::shift_month(*year, *month, forward);
                        *year = ny;
                        *month = nm;
                        // Keep the focused day inside the new (possibly shorter)
                        // month — Jan 31 → Feb lands on the 28th/29th.
                        *cursor = nokkvi_data::utils::calendar::clamp_day(ny, nm, *cursor as u32)
                            as usize;
                    }
                });
                Task::none()
            }
            RulesEditorMessage::DatePickerMoveDay { by } => {
                self.with_rules_session(|s| {
                    if let Some(SubPicker {
                        kind: SubPickerKind::DateValue { year, month, .. },
                        cursor,
                        ..
                    }) = s.sub_picker.as_mut()
                    {
                        let (ny, nm, nd) = nokkvi_data::utils::calendar::add_days(
                            *year,
                            *month,
                            *cursor as u32,
                            by,
                        );
                        *year = ny;
                        *month = nm;
                        *cursor = nd as usize;
                    }
                });
                Task::none()
            }
            RulesEditorMessage::DatePickerPickDay(day) => {
                self.commit_date_pick(day);
                Task::none()
            }
            RulesEditorMessage::DatePickerCommit => {
                let day = self
                    .rules_session()
                    .and_then(|s| match s.sub_picker.as_ref() {
                        Some(SubPicker {
                            kind: SubPickerKind::DateValue { year, month, .. },
                            cursor,
                            ..
                        }) => Some(nokkvi_data::utils::calendar::clamp_day(
                            *year,
                            *month,
                            *cursor as u32,
                        )),
                        _ => None,
                    });
                if let Some(day) = day {
                    self.commit_date_pick(day);
                }
                Task::none()
            }

            // --- JSON mode -----------------------------------------------
            RulesEditorMessage::JsonEdited(action) => {
                self.with_rules_session(|s| {
                    if let Some(json) = s.json.as_mut() {
                        json.content.perform(action);
                        json.parse_error = None;
                        json.revert_offer = false;
                        s.dirty = true;
                    }
                });
                Task::none()
            }
            RulesEditorMessage::JsonEscape => {
                self.with_rules_session_revalidate(|s| {
                    let Some(json) = s.json.as_mut() else { return };
                    let text = json.content.text();
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(value) => {
                            s.rules = SmartRules::parse(&value);
                            s.json = None;
                            s.mode = FormMode::Cursor;
                            s.rebuild_rows();
                        }
                        Err(e) => {
                            // Never silently lose work: pin the mode and
                            // offer keep-editing / revert.
                            json.parse_error = Some(e.to_string());
                            json.revert_offer = true;
                        }
                    }
                });
                Task::none()
            }
            RulesEditorMessage::JsonRevertToLastGood => {
                self.with_rules_session_revalidate(|s| {
                    if let Some(json) = s.json.take() {
                        s.rules = json.snapshot;
                        s.mode = FormMode::Cursor;
                        s.rebuild_rows();
                    }
                });
                Task::none()
            }
            RulesEditorMessage::JsonKeepEditing => {
                self.with_rules_session(|s| {
                    if let Some(json) = s.json.as_mut() {
                        json.revert_offer = false;
                    }
                });
                Task::none()
            }

            // --- preview / save ------------------------------------------
            RulesEditorMessage::Preview => self.dispatch_draft_preview(),
            // A press with unchanged rules IS the manual re-evaluate —
            // ReEvaluate stays as the observe loop's carrier.
            RulesEditorMessage::ReEvaluate => self.dispatch_draft_preview(),
            RulesEditorMessage::Save => self.handle_rules_save(false),
            RulesEditorMessage::SaveAsNew => self.handle_rules_save(true),
            RulesEditorMessage::ConfirmOverwrite => {
                // Advance the token to the server's current and re-save.
                self.with_rules_session(|s| s.save_conflict = false);
                self.handle_rules_save_skip_check()
            }
            RulesEditorMessage::ReloadServerRules => self.handle_reload_server_rules(),

            // --- create empty state --------------------------------------
            RulesEditorMessage::PresetChosen(index) => {
                self.with_rules_session_revalidate(|s| {
                    if let Some(preset) = SEED_PRESETS.get(index) {
                        s.rules = (preset.build)();
                        s.dirty = true;
                        s.mode = FormMode::Cursor;
                        s.rebuild_rows();
                        s.cursor = s
                            .rows
                            .iter()
                            .position(|r| matches!(r, FormRow::Match))
                            .unwrap_or(0);
                        s.normalize_cell();
                    }
                });
                // A preset-seeded session has real rules — the open-POST
                // runs now (blank creates stay draftless until the first
                // valid Preview press).
                self.dispatch_draft_preview()
            }
            RulesEditorMessage::StartEmpty => {
                self.with_rules_session_revalidate(|s| {
                    s.rules = SmartRules::new_empty();
                    s.dirty = true;
                    s.mode = FormMode::Cursor;
                    s.rebuild_rows();
                });
                Task::none()
            }
            RulesEditorMessage::ImportNsp => {
                Task::perform(crate::update::nsp_import::pick_nsp_file(), |result| {
                    Message::RulesEditor(RulesEditorMessage::NspParsed(result))
                })
            }
            RulesEditorMessage::EmptyStateMove { down } => {
                let count = crate::views::playlist_editor::rules_view::empty_state_entries().len();
                self.with_rules_session(|s| {
                    if count > 0 {
                        if down {
                            s.empty_state_cursor = (s.empty_state_cursor + 1).min(count - 1);
                        } else {
                            s.empty_state_cursor = s.empty_state_cursor.saturating_sub(1);
                        }
                    }
                });
                Task::none()
            }
            RulesEditorMessage::EmptyStateActivate => {
                let cursor = self.rules_session().map_or(0, |s| s.empty_state_cursor);
                let action = crate::views::playlist_editor::rules_view::empty_state_entries()
                    .into_iter()
                    .nth(cursor)
                    .map(|(_, _, msg)| msg);
                action.map_or_else(Task::none, |msg| self.update(msg))
            }
            RulesEditorMessage::NspParsed(result) => self.handle_nsp_parsed_into_session(result),
            RulesEditorMessage::SetCover => {
                match self.rules_edit_playlist_id() {
                    Some(playlist_id) => {
                        let name = self.rules_playlist_name();
                        self.handle_set_playlist_artwork(playlist_id, name)
                    }
                    None => {
                        // Create session — no server-side playlist to upload to.
                        self.toast_info("Save the playlist first to set a custom cover");
                        Task::none()
                    }
                }
            }
            RulesEditorMessage::ResetCover => match self.rules_edit_playlist_id() {
                Some(playlist_id) => {
                    let name = self.rules_playlist_name();
                    self.handle_reset_playlist_artwork(playlist_id, name)
                }
                None => Task::none(),
            },

            // --- async carriers ------------------------------------------
            RulesEditorMessage::SessionPlaylistsLoaded(list) => {
                self.with_rules_session_revalidate(|s| {
                    s.session_playlists = list;
                    s.playlists_loaded = true;
                });
                Task::none()
            }
            RulesEditorMessage::TagsDiscovered(result) => {
                self.with_rules_session_revalidate(|s| match result {
                    Ok(rows) => {
                        let discovery =
                            nokkvi_data::types::smart_criteria::TagDiscovery::from_rows(rows);
                        s.registry
                            .merge_discovered_tags(discovery.tag_names.iter().cloned());
                        s.tag_discovery = Some(discovery);
                    }
                    Err(e) => {
                        warn!("tag discovery failed: {e}");
                        s.discovery_failed = true;
                    }
                });
                Task::none()
            }
            RulesEditorMessage::PreviewLoaded {
                generation,
                source_id,
                rows,
                total,
                evaluated_at,
            } => self.handle_preview_loaded(generation, source_id, rows, total, evaluated_at),
            RulesEditorMessage::DraftPreviewLoaded {
                generation,
                draft,
                written_rules,
                rows,
                total,
                evaluated_at,
            } => {
                if generation != self.rules_preview_generation {
                    // A newer request superseded this write — the server
                    // object it minted must not leak: delete it unless it
                    // IS the session's current draft.
                    let keep = self
                        .rules_session()
                        .and_then(|s| s.draft.as_ref())
                        .is_some_and(|d| d.id == draft.id);
                    if !keep {
                        return self.delete_draft_task(draft.id);
                    }
                    return Task::none();
                }
                let source_id = draft.id.clone();
                self.with_rules_session(|s| {
                    s.draft = Some(draft);
                    s.last_written_rules = Some(written_rules);
                    s.draft_recreate_attempted = false;
                });
                self.handle_preview_loaded(generation, source_id, rows, total, evaluated_at)
            }
            RulesEditorMessage::DraftUnavailable { generation, error } => {
                if generation == self.rules_preview_generation {
                    warn!("draft create failed: {error}");
                    self.with_rules_session(|s| {
                        // Authoring-only mode: the form stays fully usable
                        // (validation is client-side); the pane offers Retry.
                        s.preview.phase = Some(PreviewPhase::Unavailable);
                    });
                }
                Task::none()
            }
            RulesEditorMessage::PreviewPageLoaded { generation, rows } => {
                if generation != self.rules_preview_generation {
                    return Task::none();
                }
                self.with_rules_session(|s| {
                    s.preview.page_loading = false;
                    let base = s.preview.rows.len();
                    s.preview.rows.extend(
                        rows.iter()
                            .enumerate()
                            .map(|(i, song)| song_to_preview_row(song, base + i)),
                    );
                    s.preview.songs.extend(rows);
                });
                self.rules_preview_artwork_tasks()
            }
            RulesEditorMessage::PlayPreviewRow => {
                // The tweak-preview-HEAR loop: play the evaluated list from
                // the centered row (SongSource::Preloaded — real songs with
                // real ids after the mediaFileId remap).
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                let Some((songs, cursor)) = self.rules_session().and_then(|s| {
                    (!s.preview.songs.is_empty())
                        .then(|| (s.preview.songs.clone(), s.preview.cursor))
                }) else {
                    return Task::none();
                };
                self.shell_action_task(
                    move |shell| async move {
                        shell
                            .play_songs(
                                songs,
                                cursor,
                                nokkvi_data::types::one_shot_shuffle::OneShotShuffle::None,
                            )
                            .await
                    },
                    Message::LoadQueue,
                    "play preview row",
                )
            }
            RulesEditorMessage::PreviewFailed { generation, error } => {
                if generation != self.rules_preview_generation {
                    return Task::none();
                }
                // Draft 404 mid-session (sweep race / external cleanup):
                // transparently recreate once and retry the preview.
                let is_404 = error.contains("status 404");
                let can_recreate = self
                    .rules_session()
                    .is_some_and(|s| s.draft.is_some() && !s.draft_recreate_attempted);
                if is_404 && can_recreate {
                    info!("draft vanished mid-session — recreating");
                    self.with_rules_session(|s| {
                        s.draft = None;
                        s.last_written_rules = None;
                        s.draft_recreate_attempted = true;
                        // Clear the paging latch — a failed page fetch set it
                        // true; the recreate re-reads from the top.
                        s.preview.page_loading = false;
                    });
                    return self.dispatch_draft_preview();
                }
                warn!("rules evaluation read failed: {error}");
                self.with_rules_session(|s| {
                    // Keep last-good rows + stamp; the pane adds the
                    // retry line (never blanks).
                    s.preview.phase = Some(PreviewPhase::Failed);
                    // A failed page fetch set page_loading — clear it or the
                    // `!page_loading` gate blocks all further paging until a
                    // full re-evaluation.
                    s.preview.page_loading = false;
                });
                Task::none()
            }
            RulesEditorMessage::PlaylistKindResolved {
                playlist_id,
                result,
            } => self.handle_playlist_kind_resolved(playlist_id, result),
            RulesEditorMessage::SaveCompleted {
                playlist_id,
                name,
                saved_updated_at,
                detached,
                spun_off,
            } => {
                if spun_off {
                    // "Save as new" FROM an edit session: the copy exists
                    // server-side, but the session keeps editing the
                    // ORIGINAL. Touch only the in-flight flags — leaving
                    // target/token/draft/dirty/metadata intact avoids both
                    // the false-conflict (adopting the copy's updatedAt) and
                    // the draft leak (dropping the live handle). The original
                    // draft stays owned by the session, cleaned on
                    // close/logout/in-place save.
                    self.with_rules_session(|s| {
                        s.saving = false;
                        s.save_conflict = false;
                        s.save_target_gone = false;
                    });
                    return Task::done(Message::PlaylistMutated(
                        crate::app_message::PlaylistMutation::RulesSaved { name },
                    ));
                }
                let had_draft = self.rules_session().is_some_and(|s| s.draft.is_some());
                self.with_rules_session(|s| {
                    s.saving = false;
                    s.dirty = false;
                    s.save_conflict = false;
                    s.save_target_gone = false;
                    // A draft-backed save already DISPLAYS the truth (the
                    // draft's evaluation); only draftless saves need the
                    // observe loop against the target.
                    s.observe_retries_left = if had_draft { 0 } else { OBSERVE_RETRIES };
                    // The create finalize consumed the draft (it BECAME the
                    // playlist); the edit path deleted it in-task.
                    if matches!(s.target, RulesTarget::Create) || had_draft {
                        s.draft = None;
                    }
                    // A create session morphs into an edit session of the
                    // new playlist — further Saves become updates and the
                    // observe loop has a target to read.
                    if matches!(s.target, RulesTarget::Create) {
                        s.target = RulesTarget::Edit {
                            playlist_id: playlist_id.clone(),
                            file_backed: false,
                            sync: false,
                            loaded_updated_at: saved_updated_at.clone(),
                        };
                    }
                });
                if let Some(editor) = self.playlist_editor.as_mut() {
                    editor.edit.mark_metadata_saved();
                    if editor.edit.playlist_id.is_empty() {
                        editor.edit.playlist_id = playlist_id.clone();
                    }
                    if !saved_updated_at.is_empty() {
                        editor.edit.set_loaded_updated_at(saved_updated_at);
                    }
                }
                if detached {
                    self.toast_info(
                        "Detached from its server-side file — the file will no longer overwrite these rules on scan",
                    );
                }
                let reload = Task::done(Message::PlaylistMutated(
                    crate::app_message::PlaylistMutation::RulesSaved { name },
                ));
                if had_draft {
                    // The pane already shows the draft's evaluation — the
                    // saved truth. No re-read.
                    reload
                } else {
                    // Draftless save: the observe loop shows the newly
                    // saved rules' matches.
                    Task::batch([reload, self.dispatch_re_evaluate()])
                }
            }
            RulesEditorMessage::SaveFailed(error) => {
                // Session preserved (nothing cleared) — the user retries or
                // discards.
                self.with_rules_session(|s| s.saving = false);
                self.toast_error(format!("Failed to save rules: {error}"));
                Task::none()
            }
            RulesEditorMessage::SaveConflict => {
                self.with_rules_session(|s| {
                    s.saving = false;
                    s.save_conflict = true;
                });
                Task::none()
            }
            RulesEditorMessage::SaveTargetGone => {
                // NOT the conflict flow: reloading a deleted playlist is a
                // dead end. Single recovery: "Save as new…".
                self.with_rules_session(|s| {
                    s.saving = false;
                    s.save_target_gone = true;
                });
                Task::none()
            }
            RulesEditorMessage::ToggleColumnVisible(col) => {
                // Optimistic flip on the persistent copy (survives editor
                // close/reopen within a session); persist through the same
                // generic path every library view's columns cog uses.
                let new_value = self.preview_column_visibility.toggle(col);
                self.persist_column_visibility(col, new_value)
            }
        }
    }

    // =====================================================================
    // Grammar helpers
    // =====================================================================

    fn handle_rules_escape(&mut self) -> Task<Message> {
        let dirty = self.rules_session().is_some_and(|s| s.dirty);
        let mut exit = false;
        self.with_rules_session(|s| {
            // The Discard chip can be clicked mid-edit — the mouse blurs the
            // value input but leaves mode == Editing. Drop out of Editing so
            // the confirm overlay's Enter/Escape reach its arm in the key
            // intercept (the Editing block would otherwise shadow the
            // confirm_discard branch, mis-routing both keys). The uncommitted
            // buffer is thrown away, which is exactly what Discard means.
            // Keyboard Escape while editing never lands here — the intercept
            // routes it to RevertEditing.
            if s.mode == FormMode::Editing {
                s.editing = None;
                s.mode = FormMode::Cursor;
            }
            if s.sub_picker.is_some() {
                s.sub_picker = None;
            } else if s.confirm_discard {
                s.confirm_discard = false;
            } else if dirty {
                s.confirm_discard = true;
            } else {
                exit = true;
            }
        });
        if exit {
            // Same in-flight-save guard as ConfirmDiscard: never delete a
            // draft the save is promoting into the real playlist.
            if self.rules_session().is_some_and(|s| s.saving) {
                self.toast_warn("Saving… — try again in a moment");
                return Task::none();
            }
            let cleanup = self
                .rules_session()
                .and_then(|s| s.draft.as_ref().map(|d| d.id.clone()))
                .map_or_else(Task::none, |id| self.delete_draft_task(id));
            Task::batch([cleanup, self.handle_exit_playlist_edit_mode()])
        } else {
            Task::none()
        }
    }

    /// Page the results pane past its loaded boundary (same evaluation —
    /// `_start > 0`), gated by the set-loading-before-fetch discipline.
    fn maybe_fetch_preview_page(&mut self) -> Task<Message> {
        let Some((source_id, start)) = self.rules_session().and_then(|s| {
            let len = s.preview.rows.len();
            let more = s.preview.total.is_some_and(|t| (t as usize) > len);
            let at_edge = len > 0 && s.preview.cursor + 1 >= len;
            (more && at_edge && !s.preview.page_loading)
                .then(|| s.preview.source_id.clone().map(|id| (id, len as u32)))
                .flatten()
        }) else {
            return Task::none();
        };
        self.with_rules_session(|s| s.preview.page_loading = true);
        let generation = self.rules_preview_generation;
        self.shell_task(
            move |shell| async move {
                let service = shell.playlists_api().await?;
                let (rows, _total) = service
                    .load_playlist_tracks_page(&source_id, start, start + PREVIEW_PAGE)
                    .await?;
                Ok(rows)
            },
            move |result: Result<Vec<nokkvi_data::types::song::Song>, anyhow::Error>| match result {
                Ok(rows) => {
                    Message::RulesEditor(RulesEditorMessage::PreviewPageLoaded { generation, rows })
                }
                Err(e) => Message::RulesEditor(RulesEditorMessage::PreviewFailed {
                    generation,
                    error: format!("{e:#}"),
                }),
            },
        )
    }

    /// Left/Right in cursor mode: edit-bar steps columns; enum cells cycle
    /// their value through its const ALL (the one write path shared with
    /// mouse).
    fn handle_cycle_cell(&mut self, forward: bool) -> Task<Message> {
        self.with_rules_session_revalidate(|s| {
            if s.mode != FormMode::Cursor || s.pane != RulesPane::Form {
                return;
            }
            let Some(row) = s.rows.get(s.cursor).cloned() else {
                return;
            };
            match (&row, s.cell) {
                (FormRow::EditBar, _) => s.step_edit_bar(forward),
                (FormRow::Match, FormCell::ConjunctionPill) => {
                    if let Some(root) = s.rules.root.as_mut() {
                        root.conjunction = match root.conjunction {
                            Conjunction::All => Conjunction::Any,
                            Conjunction::Any => Conjunction::All,
                        };
                        root.original_key = root.conjunction.wire_key().to_owned();
                        s.dirty = true;
                    }
                }
                (FormRow::GroupHeader(path), FormCell::ConjunctionPill) => {
                    if let Some(CriteriaNode::Group(group)) = s.node_at_mut(path) {
                        group.conjunction = match group.conjunction {
                            Conjunction::All => Conjunction::Any,
                            Conjunction::Any => Conjunction::All,
                        };
                        group.original_key = group.conjunction.wire_key().to_owned();
                        s.dirty = true;
                    }
                }
                (FormRow::Rule(path), FormCell::Field) => {
                    if !s.form_editable() {
                        return;
                    }
                    let next = {
                        let Some(CriteriaNode::Leaf(leaf)) = s.node_at(path) else {
                            return;
                        };
                        cycle_slice(
                            nokkvi_data::types::smart_criteria::PICKER_FIELDS,
                            &leaf.field,
                            forward,
                        )
                    };
                    s.set_leaf_field(path, next);
                }
                (FormRow::Rule(path), FormCell::Operator) => {
                    if !s.form_editable() {
                        return;
                    }
                    let ops = s.valid_operators_for_row(path);
                    let next = {
                        let Some(CriteriaNode::Leaf(leaf)) = s.node_at(path) else {
                            return;
                        };
                        cycle_items(&ops, &leaf.operator, forward)
                    };
                    if let Some(op) = next {
                        s.set_leaf_operator(path, op);
                    }
                }
                (FormRow::Rule(path), FormCell::Value) => {
                    // Toggle cells cycle On/Off with Left/Right too.
                    let is_toggle = matches!(s.value_shape_at(path), Some(ValueShape::Toggle));
                    if is_toggle && s.form_editable() {
                        s.toggle_leaf_bool(path);
                    }
                }
                (FormRow::SortKey(i), FormCell::SortDirection) => {
                    let mut keys = s.rules.effective_sort_keys();
                    if let Some(key) = keys.get_mut(*i) {
                        key.descending = !key.descending;
                        s.rules.edit_sort(keys);
                        s.dirty = true;
                        s.rebuild_rows();
                    }
                }
                (FormRow::SortKey(i), FormCell::SortField) => {
                    let mut keys = s.rules.effective_sort_keys();
                    if let Some(key) = keys.get_mut(*i) {
                        key.field = cycle_slice(SORT_QUICK_FIELDS, &key.field, forward).to_owned();
                        s.rules.edit_sort(keys);
                        s.dirty = true;
                        s.rebuild_rows();
                    }
                }
                (FormRow::Limit, FormCell::LimitMode) => {
                    // Flip #/% keeping the number.
                    let (limit, pct) = (s.rules.limit, s.rules.limit_percent);
                    match (limit, pct) {
                        (Some(n), _) => {
                            s.rules.limit = None;
                            s.rules.limit_percent = Some(n.clamp(1, 100));
                        }
                        (None, Some(p)) => {
                            s.rules.limit_percent = None;
                            s.rules.limit = Some(p);
                        }
                        (None, None) => s.rules.limit = Some(100),
                    }
                    s.dirty = true;
                }
                _ => {
                    // Non-cyclable cells: Left/Right steps like Tab so the
                    // hand never dead-ends.
                    if forward {
                        s.step_cell();
                    }
                }
            }
        });
        Task::none()
    }

    /// The playlist id of an EDIT rules session (None on a create session,
    /// which has no server-side playlist yet).
    fn rules_edit_playlist_id(&self) -> Option<String> {
        self.rules_session().and_then(|s| match &s.target {
            RulesTarget::Edit { playlist_id, .. } => Some(playlist_id.clone()),
            RulesTarget::Create => None,
        })
    }

    /// The editor's current playlist name (for the artwork toasts).
    fn rules_playlist_name(&self) -> String {
        self.playlist_editor
            .as_ref()
            .map(|e| e.edit.playlist_name.clone())
            .unwrap_or_default()
    }

    /// The one-shot focus op for the session's CURRENT editable, if any:
    /// JSON editor > sub-picker search > the Editing-mode input (edit-bar
    /// name/comment, else the value cell). Emitted by every transition into
    /// an editable so keyboard-only authoring never needs a priming click.
    /// `Task::none()` in Cursor mode — the form owns keys via the intercept.
    fn rules_focus_task(&self) -> Task<Message> {
        use iced::widget::operation::focus;

        use crate::views::playlist_editor::rules_view::{
            RULES_COMMENT_INPUT_ID, RULES_JSON_EDITOR_ID, RULES_NAME_INPUT_ID,
            RULES_SUB_PICKER_INPUT_ID, RULES_VALUE_INPUT_ID,
        };
        let Some(s) = self.rules_session() else {
            return Task::none();
        };
        if s.json.is_some() {
            return focus(RULES_JSON_EDITOR_ID);
        }
        if s.sub_picker.is_some() {
            return focus(RULES_SUB_PICKER_INPUT_ID);
        }
        if s.mode == FormMode::Editing {
            return match s.editing.as_ref().map(|e| e.cell) {
                Some(FormCell::Name) => focus(RULES_NAME_INPUT_ID),
                Some(FormCell::Comment) => focus(RULES_COMMENT_INPUT_ID),
                _ => focus(RULES_VALUE_INPUT_ID),
            };
        }
        Task::none()
    }

    /// Enter in cursor mode — the per-cell action.
    fn handle_enter_on_cursor(&mut self) -> Task<Message> {
        let mut save_as_new = false;
        let mut toggle_public = false;
        // Snapshot the edit-bar text BEFORE entering Editing so Escape can
        // restore it (the name/comment inputs live-commit each keystroke to
        // editor.edit — there is no buffer to drop). Captured out here since
        // the session closure can't reach `playlist_editor`.
        let (cur_name, cur_comment) = self
            .playlist_editor
            .as_ref()
            .map(|e| {
                (
                    e.edit.playlist_name.clone(),
                    e.edit.playlist_comment.clone(),
                )
            })
            .unwrap_or_default();
        self.with_rules_session(|s| {
            if s.pane != RulesPane::Form || s.mode != FormMode::Cursor {
                return;
            }
            let Some(row) = s.rows.get(s.cursor).cloned() else {
                return;
            };
            match (&row, s.cell) {
                (FormRow::EditBar, FormCell::Name | FormCell::Comment) => {
                    // The edit-bar inputs live-commit via EditorMessage; the
                    // Editing mode records the revert snapshot so Escape can
                    // put the pre-edit text back.
                    let revert = if s.cell == FormCell::Comment {
                        cur_comment.clone()
                    } else {
                        cur_name.clone()
                    };
                    s.mode = FormMode::Editing;
                    s.editing = Some(EditingCell {
                        row: row.clone(),
                        cell: s.cell,
                        buffer: String::new(),
                        revert,
                    });
                }
                (FormRow::EditBar, FormCell::Public) => {
                    // The public flag lives on `editor.edit`, not the session,
                    // so the flip happens after the closure — through the same
                    // `set_public` write path the mouse pill uses.
                    toggle_public = true;
                }
                (FormRow::EditBar, FormCell::SaveAsNew) => save_as_new = true,
                (FormRow::Match | FormRow::GroupHeader(_), FormCell::ConjunctionPill) => {
                    // Enter cycles the pill (same as Left/Right).
                }
                (FormRow::Rule(path), FormCell::Field) if s.form_editable() => {
                    s.sub_picker = Some(SubPicker {
                        kind: SubPickerKind::Field { row: row.clone() },
                        query: String::new(),
                        cursor: 0,
                    });
                    let _ = path;
                }
                (FormRow::Rule(path), FormCell::Operator) if s.form_editable() => {
                    s.sub_picker = Some(SubPicker {
                        kind: SubPickerKind::Operator { row: row.clone() },
                        query: String::new(),
                        cursor: 0,
                    });
                    let _ = path;
                }
                (FormRow::Rule(path), FormCell::Value | FormCell::Value2) => {
                    if !s.form_editable() {
                        return;
                    }
                    // A tag value (genre/mood/…) is PICKED from the library's
                    // values, not typed — server matching is case-sensitive
                    // ("phonk" ≠ "Phonk"). Computed before the match's mutable
                    // borrows.
                    let tag_pick = s.tag_for_value_picker(path);
                    let rating_pick = s.is_single_rating_value(path);
                    match s.value_shape_at(path) {
                        Some(ValueShape::Toggle) => s.toggle_leaf_bool(path),
                        Some(ValueShape::PlaylistRef) => {
                            s.sub_picker = Some(SubPicker {
                                kind: SubPickerKind::Playlist { row: row.clone() },
                                query: String::new(),
                                cursor: 0,
                            });
                        }
                        Some(ValueShape::Date | ValueShape::DatePair) => {
                            // A date is PICKED off a calendar grid, not typed —
                            // the server does a raw string compare, so it must
                            // be an exact YYYY-MM-DD. Seed the displayed month +
                            // focused day from the cell's current date, else
                            // today (an empty/unset cell reads back as "").
                            let slot2 = s.cell == FormCell::Value2;
                            let current = s.leaf_value_text(path, slot2);
                            let (year, month, day) =
                                nokkvi_data::utils::calendar::parse_ymd(&current)
                                    .unwrap_or_else(nokkvi_data::utils::calendar::today_ymd);
                            s.sub_picker = Some(SubPicker {
                                kind: SubPickerKind::DateValue {
                                    row: row.clone(),
                                    slot2,
                                    year,
                                    month,
                                },
                                query: String::new(),
                                cursor: day as usize,
                            });
                        }
                        Some(_) if tag_pick.is_some() => {
                            s.sub_picker = Some(SubPicker {
                                kind: SubPickerKind::TagValue {
                                    row: row.clone(),
                                    tag: tag_pick.unwrap_or_default(),
                                },
                                query: String::new(),
                                cursor: 0,
                            });
                        }
                        Some(_) if rating_pick => {
                            s.sub_picker = Some(SubPicker {
                                kind: SubPickerKind::RatingValue { row: row.clone() },
                                query: String::new(),
                                cursor: 0,
                            });
                        }
                        Some(_) => {
                            let slot2 = s.cell == FormCell::Value2;
                            let current = s.leaf_value_text(path, slot2);
                            s.mode = FormMode::Editing;
                            s.editing = Some(EditingCell {
                                row: row.clone(),
                                cell: s.cell,
                                buffer: current.clone(),
                                revert: current,
                            });
                        }
                        None => {}
                    }
                }
                (FormRow::Rule(path), FormCell::Remove) if s.form_editable() => {
                    s.remove_node(path);
                }
                (FormRow::AddRule(path), _) if s.form_editable() => {
                    s.add_rule(path);
                }
                (FormRow::AddGroup, _) if s.form_editable() => {
                    s.add_group();
                }
                (FormRow::SortKey(i), FormCell::SortField) => {
                    s.sub_picker = Some(SubPicker {
                        kind: SubPickerKind::SortField { index: *i },
                        query: String::new(),
                        cursor: 0,
                    });
                }
                (FormRow::SortKey(i), FormCell::SortDirection) => {
                    let mut keys = s.rules.effective_sort_keys();
                    if let Some(key) = keys.get_mut(*i) {
                        key.descending = !key.descending;
                        s.rules.edit_sort(keys);
                        s.dirty = true;
                        s.rebuild_rows();
                    }
                }
                (FormRow::SortKey(i), FormCell::Remove) => s.remove_sort_key(*i),
                (FormRow::AddSortKey, _) => s.add_sort_key(),
                (FormRow::Limit, FormCell::LimitValue | FormCell::OffsetValue) => {
                    let is_offset = s.cell == FormCell::OffsetValue;
                    let current = if is_offset {
                        s.rules.offset.map(|n| n.to_string()).unwrap_or_default()
                    } else {
                        s.rules
                            .limit
                            .or(s.rules.limit_percent)
                            .map(|n| n.to_string())
                            .unwrap_or_default()
                    };
                    s.mode = FormMode::Editing;
                    s.editing = Some(EditingCell {
                        row: row.clone(),
                        cell: s.cell,
                        buffer: current.clone(),
                        revert: current,
                    });
                }
                (FormRow::Limit, FormCell::LimitMode) => {
                    // Same flip as Left/Right (shared write path).
                }
                (FormRow::JsonToggle, _) => {
                    // Enter JSON mode with the editor focused + snapshot
                    // taken (the round-3 pinned transition).
                    let pretty = serde_json::to_string_pretty(&s.rules.to_value())
                        .unwrap_or_else(|_| "{}".to_owned());
                    s.json = Some(JsonModeState {
                        content: iced::widget::text_editor::Content::with_text(&pretty),
                        snapshot: s.rules.clone(),
                        parse_error: None,
                        revert_offer: false,
                    });
                    s.mode = FormMode::Json;
                }
                _ => {}
            }
        });
        // Enter on the Public cell flips visibility through the shared
        // `set_public` write path (keyboard parity with the mouse pill).
        if toggle_public && let Some(editor) = self.playlist_editor.as_mut() {
            let next = !editor.edit.playlist_public;
            editor.edit.set_public(next);
        }
        let follow_up = if save_as_new {
            self.handle_rules_save(true)
        } else {
            Task::none()
        };
        // Focus whatever input this transition activated (value cell,
        // sub-picker search, JSON editor, or edit-bar name/comment) so
        // keyboard authoring never needs a priming click. Computed here
        // after the mutation, before the mutable re-borrows below.
        let focus = self.rules_focus_task();
        // The pill cells share the cycle write path.
        let cycles = self.rules_session().is_some_and(|s| {
            s.mode == FormMode::Cursor
                && matches!(
                    (s.rows.get(s.cursor), s.cell),
                    (
                        Some(FormRow::Match | FormRow::GroupHeader(_)),
                        FormCell::ConjunctionPill
                    ) | (Some(FormRow::Limit), FormCell::LimitMode)
                )
        });
        if cycles {
            return Task::batch([follow_up, self.handle_cycle_cell(true), focus]);
        }
        self.revalidate_rules_session();
        Task::batch([follow_up, focus])
    }

    /// Commit the Editing-mode buffer into the rules (value/limit cells;
    /// the edit-bar inputs live-commit through EditorMessage).
    fn commit_editing_cell(&mut self) {
        self.with_rules_session(|s| {
            let Some(editing) = s.editing.take() else {
                s.mode = FormMode::Cursor;
                return;
            };
            s.mode = FormMode::Cursor;
            match (&editing.row, editing.cell) {
                (FormRow::Rule(path), FormCell::Value | FormCell::Value2) => {
                    let slot2 = editing.cell == FormCell::Value2;
                    s.set_leaf_value_text(path, &editing.buffer, slot2);
                }
                (FormRow::Limit, FormCell::LimitValue) => {
                    let n = editing.buffer.trim().parse::<u64>().ok();
                    if s.rules.limit_percent.is_some() {
                        s.rules.limit_percent = n;
                    } else {
                        s.rules.limit = n;
                    }
                    s.dirty = true;
                }
                (FormRow::Limit, FormCell::OffsetValue) => {
                    s.rules.offset = editing.buffer.trim().parse::<u64>().ok();
                    s.dirty = true;
                }
                // Edit-bar cells: nothing to write — EditorMessage already
                // committed each keystroke.
                _ => {}
            }
        });
    }

    /// Flush an in-progress value/limit edit before an action that reads
    /// `session.rules` (Save / Preview / Save-as-new). A mouse click on an
    /// edit-bar action chip blurs the focused input WITHOUT committing it —
    /// iced has no blur-commit — so without this the chip reads the pre-edit
    /// rules: silently persisting the old value (clearing dirty) or
    /// previewing stale results. Keyboard Save/Preview are already safe (the
    /// key intercept commits first); this closes the mouse path. A no-op
    /// unless a cell is mid-edit.
    fn commit_pending_edit(&mut self) {
        if self
            .rules_session()
            .is_some_and(|s| s.mode == FormMode::Editing)
        {
            self.commit_editing_cell();
            self.revalidate_rules_session();
        }
    }

    /// Commit the date picker's chosen `day` into the leaf value slot, then
    /// close the picker. Formats to the exact `YYYY-MM-DD` the criteria wire
    /// requires and reuses `set_leaf_value_text`, so validation is identical
    /// to a hand-typed date. A no-op unless a DateValue picker is open on a
    /// rule row.
    fn commit_date_pick(&mut self, day: u32) {
        let picked = self
            .rules_session()
            .and_then(|s| match s.sub_picker.as_ref() {
                Some(SubPicker {
                    kind:
                        SubPickerKind::DateValue {
                            row: FormRow::Rule(path),
                            slot2,
                            year,
                            month,
                        },
                    ..
                }) => Some((
                    path.clone(),
                    *slot2,
                    nokkvi_data::utils::calendar::format_ymd(*year, *month, day),
                )),
                _ => None,
            });
        if let Some((path, slot2, text)) = picked {
            self.with_rules_session(|s| {
                s.set_leaf_value_text(&path, &text, slot2);
                s.sub_picker = None;
            });
            self.revalidate_rules_session();
        }
    }

    fn commit_sub_picker(&mut self) {
        let Some((kind, entry)) = self.rules_session().and_then(|s| {
            let picker = s.sub_picker.as_ref()?;
            let entries = rules_picker_entries(s, picker);
            let entry = entries.get(picker.cursor)?.clone();
            Some((picker.kind.clone(), entry))
        }) else {
            self.with_rules_session(|s| s.sub_picker = None);
            return;
        };
        self.with_rules_session(|s| {
            s.sub_picker = None;
            match kind {
                SubPickerKind::Field {
                    row: FormRow::Rule(path),
                } => {
                    s.set_leaf_field(&path, &entry.0);
                }
                SubPickerKind::Operator {
                    row: FormRow::Rule(path),
                } => {
                    if let Some(op) = RuleOperator::from_wire_key(&entry.0) {
                        s.set_leaf_operator(&path, op);
                    }
                }
                SubPickerKind::Playlist {
                    row: FormRow::Rule(path),
                } => {
                    if let Some(CriteriaNode::Leaf(leaf)) = s.node_at_mut(&path) {
                        leaf.field = "id".to_owned();
                        leaf.value = serde_json::Value::String(entry.0.clone());
                        s.dirty = true;
                    }
                }
                SubPickerKind::TagValue {
                    row: FormRow::Rule(path),
                    ..
                } => {
                    if let Some(CriteriaNode::Leaf(leaf)) = s.node_at_mut(&path) {
                        leaf.value = serde_json::Value::String(entry.0.clone());
                        s.dirty = true;
                    }
                }
                SubPickerKind::RatingValue {
                    row: FormRow::Rule(path),
                } => {
                    if let Some(CriteriaNode::Leaf(leaf)) = s.node_at_mut(&path) {
                        // Ratings are NUMBERS on the wire, not strings.
                        leaf.value = serde_json::json!(entry.0.parse::<i64>().unwrap_or_default());
                        s.dirty = true;
                    }
                }
                SubPickerKind::SortField { index } => {
                    let mut keys = s.rules.effective_sort_keys();
                    if let Some(key) = keys.get_mut(index) {
                        key.field = entry.0.clone();
                        s.rules.edit_sort(keys);
                        s.dirty = true;
                        s.rebuild_rows();
                    }
                }
                _ => {}
            }
        });
    }
}

/// The (value, label) entries of the open sub-picker, filtered by its
/// immediate search query. Free + pure: the handler and the view both
/// derive rows from it (render/commit parity).
pub(crate) fn rules_picker_entries(
    session: &RulesSessionUi,
    picker: &SubPicker,
) -> Vec<(String, String)> {
    let query = picker.query.to_lowercase();
    let matches = |label: &str, value: &str| {
        query.is_empty()
            || label.to_lowercase().contains(&query)
            || value.to_lowercase().contains(&query)
    };
    match &picker.kind {
        SubPickerKind::Field { .. } => {
            let mut entries: Vec<(String, String)> = Vec::new();
            // Quick rows first (evidence-ranked), then the rest of the
            // whitelist — "More fields…" as one searchable list.
            for name in nokkvi_data::types::smart_criteria::PICKER_FIELDS {
                if let Some(label) = field_label(&session.registry, name)
                    && matches(&label, name)
                {
                    entries.push(((*name).to_owned(), label));
                }
            }
            for def in nokkvi_data::types::smart_criteria::STATIC_FIELDS {
                if !nokkvi_data::types::smart_criteria::PICKER_FIELDS.contains(&def.name)
                    && matches(def.label, def.name)
                {
                    entries.push((def.name.to_owned(), def.label.to_owned()));
                }
            }
            for role in nokkvi_data::types::smart_criteria::ROLE_FIELDS {
                if !nokkvi_data::types::smart_criteria::PICKER_FIELDS.contains(&role)
                    && matches(role, role)
                {
                    entries.push(((*role).to_owned(), format!("{role} (role)")));
                }
            }
            for tag in &session.registry.discovered_tags {
                let known_static = nokkvi_data::types::smart_criteria::STATIC_FIELDS
                    .iter()
                    .any(|d| d.name == tag.as_str());
                if !known_static
                    && !nokkvi_data::types::smart_criteria::PICKER_FIELDS.contains(&tag.as_str())
                    && matches(tag, tag)
                {
                    entries.push((tag.clone(), format!("{tag} (tag)")));
                }
            }
            entries
        }
        SubPickerKind::Operator { row } => {
            let FormRow::Rule(path) = row else {
                return Vec::new();
            };
            session
                .valid_operators_for_row(path)
                .into_iter()
                .filter(|op| matches(op.label(), op.wire_key()))
                .map(|op| (op.wire_key().to_owned(), op.label().to_owned()))
                .collect()
        }
        SubPickerKind::Playlist { .. } => {
            let target_id = match &session.target {
                RulesTarget::Edit { playlist_id, .. } => Some(playlist_id.as_str()),
                RulesTarget::Create => None,
            };
            session
                .session_playlists
                .iter()
                .filter(|(id, name)| Some(id.as_str()) != target_id && matches(name, id))
                .map(|(id, name)| (id.clone(), name.clone()))
                .collect()
        }
        SubPickerKind::SortField { .. } => {
            let mut entries: Vec<(String, String)> = Vec::new();
            for def in nokkvi_data::types::smart_criteria::STATIC_FIELDS {
                if matches(def.label, def.name) {
                    entries.push((def.name.to_owned(), def.label.to_owned()));
                }
            }
            if matches("random", "random") {
                entries.push(("random".to_owned(), "Random (flagged)".to_owned()));
            }
            entries
        }
        SubPickerKind::TagValue { tag, .. } => {
            let mut entries: Vec<(String, String)> = session
                .tag_discovery
                .as_ref()
                .and_then(|d| d.values_by_tag.get(tag))
                .map(|values| {
                    values
                        .iter()
                        .filter(|v| matches(v, v))
                        .map(|v| (v.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            // Never trap the user: a non-empty query that isn't an exact
            // library value can still be committed verbatim (a value not yet
            // scanned, or a `contains` substring).
            let q = picker.query.trim();
            if !q.is_empty() && !entries.iter().any(|(v, _)| v == q) {
                entries.insert(0, (q.to_owned(), format!("Use \"{q}\"")));
            }
            entries
        }
        SubPickerKind::RatingValue { .. } => (0..=5)
            .map(|n| {
                let label = match n {
                    0 => "Unrated".to_owned(),
                    1 => "1 star".to_owned(),
                    _ => format!("{n} stars"),
                };
                (n.to_string(), label)
            })
            .filter(|(value, label)| matches(label, value))
            .collect(),
        // The date picker renders a calendar grid, not a filterable list — it
        // carries no entries and commits from its own year/month/day state.
        SubPickerKind::DateValue { .. } | SubPickerKind::None => Vec::new(),
    }
}

impl Nokkvi {
    // =====================================================================
    // Evaluation reads (M4 interim) + observe loop
    // =====================================================================

    /// The read-only tracks+meta GET of the session's evaluation SOURCE —
    /// the draft when one exists, else the edit target. Never writes rules
    /// (the unchanged-rules re-press and the observe loop both land here:
    /// outside the refresh window the first-page read re-evaluates;
    /// inside it the server serves the stored same-rules result — honest
    /// either way, and never force-touched).
    fn dispatch_re_evaluate(&mut self) -> Task<Message> {
        let Some(playlist_id) = self.rules_session().and_then(|s| {
            s.draft
                .as_ref()
                .map(|d| d.id.clone())
                .or_else(|| match &s.target {
                    RulesTarget::Edit { playlist_id, .. } => Some(playlist_id.clone()),
                    RulesTarget::Create => None,
                })
        }) else {
            return Task::none();
        };
        self.rules_preview_generation = self.rules_preview_generation.wrapping_add(1);
        let generation = self.rules_preview_generation;
        self.with_rules_session(|s| {
            s.captured_generation = generation;
            s.preview.phase = Some(PreviewPhase::Evaluating);
        });
        let source_for_msg = playlist_id.clone();
        self.shell_task(
            move |shell| async move {
                let service = shell.playlists_api().await?;
                // page 0 triggers the owner-side lazy refresh; the meta read
                // then carries the fresh evaluatedAt.
                let (songs, total) = service
                    .load_playlist_tracks_page(&playlist_id, 0, PREVIEW_PAGE)
                    .await?;
                let meta = service.get_playlist(&playlist_id).await?;
                Ok((songs, total, meta.evaluated_at))
            },
            move |result: Result<EvalPayload, anyhow::Error>| match result {
                Ok((rows, total, evaluated_at)) => {
                    Message::RulesEditor(RulesEditorMessage::PreviewLoaded {
                        generation,
                        source_id: source_for_msg.clone(),
                        rows,
                        total,
                        evaluated_at,
                    })
                }
                Err(e) => Message::RulesEditor(RulesEditorMessage::PreviewFailed {
                    generation,
                    error: format!("{e:#}"),
                }),
            },
        )
    }

    /// The draft-workspace preview engine (the ruled M5 end state).
    ///
    /// Freshness on RELEASED servers is guaranteed by construction: a
    /// changed-rules press deletes + recreates the draft (a fresh playlist
    /// has nil EvaluatedAt, so its first-page read always evaluates); the
    /// atomic PUT path activates only when caps confirm the PUT-nil
    /// behavior. No clock comparisons, no polling, never a count the
    /// server didn't produce.
    fn dispatch_draft_preview(&mut self) -> Task<Message> {
        // A save in flight is finalizing / owns the draft (a create-save
        // promotes the draft id into the real playlist) — a concurrent
        // delete+recreate preview would race it. Let the save settle first.
        if self.rules_session().is_some_and(|s| s.saving) {
            return Task::none();
        }
        // A mouse click on Preview blurs the focused value input without
        // committing it — flush the pending edit so we preview the typed
        // rules, not the pre-edit ones (stale results otherwise).
        self.commit_pending_edit();
        // Validation gates FIRST (client-side — no server needed to refuse
        // honestly), then the shell check.
        self.revalidate_rules_session();
        let Some(session) = self.rules_session() else {
            return Task::none();
        };
        if session.has_blocking_errors() {
            self.toast_warn("Fix the marked rule to preview");
            return Task::none();
        }
        if self.app_service.is_none() {
            return Task::none();
        }
        let Some(session) = self.rules_session() else {
            return Task::none();
        };
        // Structural guarantee: the serializer is unreachable for an empty
        // root (the validation gate above) — no POST/PUT body can carry an
        // empty conjunction.
        let rules_value = session.rules.to_value();
        let unchanged = session.last_written_rules.as_ref() == Some(&rules_value);
        let draft = session.draft.clone();
        let caps = session.caps;

        if unchanged && draft.is_some() {
            // Read-only re-press of the same rules.
            return self.dispatch_re_evaluate();
        }

        self.rules_preview_generation = self.rules_preview_generation.wrapping_add(1);
        let generation = self.rules_preview_generation;
        self.with_rules_session(|s| {
            s.captured_generation = generation;
            s.preview.phase = Some(PreviewPhase::Evaluating);
        });
        let marker = mint_draft_marker();

        match draft {
            // Changed rules on a caps-confirmed PUT-nil server: the atomic
            // PUT-then-read.
            Some(d) if caps.put_nils_evaluated_at => {
                let draft_id = d.id.clone();
                let marker_for_msg = marker.clone();
                let rules_for_msg = rules_value.clone();
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service
                            .put_playlist_full(
                                &draft_id,
                                nokkvi_data::types::rules_session::DRAFT_DISPLAY_NAME,
                                &marker,
                                false,
                                Some(&rules_value),
                                None,
                            )
                            .await?;
                        let (rows, total) = service
                            .load_playlist_tracks_page(&draft_id, 0, PREVIEW_PAGE)
                            .await?;
                        let meta = service.get_playlist(&draft_id).await?;
                        Ok((draft_id, rows, total, meta.evaluated_at))
                    },
                    move |result: Result<DraftEvalPayload, anyhow::Error>| match result {
                        Ok((id, rows, total, evaluated_at)) => {
                            Message::RulesEditor(RulesEditorMessage::DraftPreviewLoaded {
                                generation,
                                draft: nokkvi_data::types::rules_session::DraftInfo {
                                    id,
                                    marker: marker_for_msg.clone(),
                                },
                                written_rules: rules_for_msg.clone(),
                                rows,
                                total,
                                evaluated_at,
                            })
                        }
                        Err(e) => Message::RulesEditor(RulesEditorMessage::PreviewFailed {
                            generation,
                            error: format!("{e:#}"),
                        }),
                    },
                )
            }
            // Changed rules on every RELEASED server today: delete +
            // recreate — deterministic freshness by the create-path nil.
            Some(d) => {
                let old_id = d.id.clone();
                let marker_for_msg = marker.clone();
                let rules_for_msg = rules_value.clone();
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        // 404 tolerated — the sweep or an external cleanup
                        // may have beaten us to it.
                        let _ = service.delete_playlist(&old_id).await;
                        let id = service
                            .create_smart_playlist(
                                nokkvi_data::types::rules_session::DRAFT_DISPLAY_NAME,
                                &marker,
                                false,
                                &rules_value,
                            )
                            .await?;
                        let (rows, total) = service
                            .load_playlist_tracks_page(&id, 0, PREVIEW_PAGE)
                            .await?;
                        let meta = service.get_playlist(&id).await?;
                        Ok((id, rows, total, meta.evaluated_at))
                    },
                    move |result: Result<DraftEvalPayload, anyhow::Error>| match result {
                        Ok((id, rows, total, evaluated_at)) => {
                            Message::RulesEditor(RulesEditorMessage::DraftPreviewLoaded {
                                generation,
                                draft: nokkvi_data::types::rules_session::DraftInfo {
                                    id,
                                    marker: marker_for_msg.clone(),
                                },
                                written_rules: rules_for_msg.clone(),
                                rows,
                                total,
                                evaluated_at,
                            })
                        }
                        Err(e) => Message::RulesEditor(RulesEditorMessage::PreviewFailed {
                            generation,
                            error: format!("{e:#}"),
                        }),
                    },
                )
            }
            // First draft write: session open (rules-bearing targets) or a
            // blank create's first validation-passing Preview press.
            None => {
                let marker_for_msg = marker.clone();
                let rules_for_msg = rules_value.clone();
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        let id = service
                            .create_smart_playlist(
                                nokkvi_data::types::rules_session::DRAFT_DISPLAY_NAME,
                                &marker,
                                false,
                                &rules_value,
                            )
                            .await?;
                        let (rows, total) = service
                            .load_playlist_tracks_page(&id, 0, PREVIEW_PAGE)
                            .await?;
                        let meta = service.get_playlist(&id).await?;
                        Ok((id, rows, total, meta.evaluated_at))
                    },
                    move |result: Result<DraftEvalPayload, anyhow::Error>| match result {
                        Ok((id, rows, total, evaluated_at)) => {
                            Message::RulesEditor(RulesEditorMessage::DraftPreviewLoaded {
                                generation,
                                draft: nokkvi_data::types::rules_session::DraftInfo {
                                    id,
                                    marker: marker_for_msg.clone(),
                                },
                                written_rules: rules_for_msg.clone(),
                                rows,
                                total,
                                evaluated_at,
                            })
                        }
                        Err(e) => Message::RulesEditor(RulesEditorMessage::DraftUnavailable {
                            generation,
                            error: format!("{e:#}"),
                        }),
                    },
                )
            }
        }
    }

    /// The startup orphan sweep — once per auth (including the re-login
    /// after a 401, which is exactly when arm (b) fires: a 401 teardown
    /// couldn't authorize its draft DELETE). Uses the RAW list loader (the
    /// filtered default would hide its own targets).
    pub(crate) fn orphan_sweep_task(
        &self,
        shell: nokkvi_data::backend::app_service::AppService,
    ) -> Task<Message> {
        let live_draft_id = self
            .rules_session()
            .and_then(|s| s.draft.as_ref().map(|d| d.id.clone()));
        Task::perform(
            async move {
                let Ok(api) = shell.playlists_api().await else {
                    return;
                };
                let rows = match api.load_playlists_with_libraries_raw().await {
                    Ok(rows) => rows,
                    Err(e) => {
                        warn!("orphan sweep list fetch failed: {e:#}");
                        return;
                    }
                };
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let ids = nokkvi_data::services::api::playlists::select_sweepable_drafts(
                    &rows,
                    std::process::id(),
                    live_draft_id.as_deref(),
                    now,
                    |pid| std::path::Path::new(&format!("/proc/{pid}")).exists(),
                );
                for id in ids {
                    match api.delete_playlist(&id).await {
                        Ok(()) => info!(" orphan sweep deleted stray draft {id}"),
                        Err(e) => warn!("orphan sweep delete failed for {id}: {e:#}"),
                    }
                }
            },
            |()| Message::NoOp,
        )
    }

    /// Best-effort draft deletion (cancel / discard / stale-write cleanup).
    /// The startup orphan sweep is the backstop for every failure here.
    fn delete_draft_task(&self, draft_id: String) -> Task<Message> {
        let Some(shell) = self.app_service.clone() else {
            return Task::none();
        };
        Task::perform(
            async move {
                if let Ok(api) = shell.playlists_api().await {
                    let _ = api.delete_playlist(&draft_id).await;
                }
            },
            |()| Message::NoOp,
        )
    }

    /// Prefetch 80px cover art for the loaded preview rows into the shared
    /// album-art cache the preview pane reads. Rules preview rows are NOT the
    /// Tracks editor's `editor.songs` (empty here) and carry no artwork_url,
    /// so the slot-list/url prefetch (`editor_artwork_prefetch_tasks`) can't
    /// serve them — fetch by album_id instead. Deduped, skips already-cached
    /// and empty ids; bounded by the loaded page count.
    fn rules_preview_artwork_tasks(&self) -> Task<Message> {
        use crate::app_message::{ArtworkMessage, MiniArt};
        let (Some(shell), Some(session)) = (self.app_service.as_ref(), self.rules_session()) else {
            return Task::none();
        };
        let mut seen = std::collections::HashSet::new();
        let mut tasks = Vec::new();
        for song in session.preview.rows.iter() {
            let id = &song.album_id;
            if id.is_empty()
                || self.artwork.album_art.snapshot.contains_key(id)
                || !seen.insert(id.clone())
            {
                continue;
            }
            let albums = shell.albums().clone();
            let album_id = id.clone();
            tasks.push(Task::perform(
                async move {
                    let art = MiniArt::from_fetch(
                        albums.fetch_album_artwork(&album_id, Some(80), None).await,
                    );
                    (album_id, art)
                },
                |(id, art)| Message::Artwork(ArtworkMessage::Loaded(id, None, art)),
            ));
        }
        Task::batch(tasks)
    }

    fn handle_preview_loaded(
        &mut self,
        generation: u64,
        source_id: String,
        rows: Vec<nokkvi_data::types::song::Song>,
        total: Option<u32>,
        evaluated_at: Option<String>,
    ) -> Task<Message> {
        if generation != self.rules_preview_generation {
            debug!("dropping stale rules evaluation (gen {generation})");
            return Task::none();
        }
        let mut needs_repoll = false;
        self.with_rules_session(|s| {
            s.preview.source_id = Some(source_id);
            s.preview.page_loading = false;
            s.draft_recreate_attempted = false;
            let ui_rows: Vec<_> = rows
                .iter()
                .enumerate()
                .map(|(i, song)| song_to_preview_row(song, i))
                .collect();
            // The bounded post-save observe loop: while the stamp hasn't
            // advanced past the pre-save one, schedule another read.
            if s.observe_retries_left > 0 {
                let advanced = match (&s.stamp_before_save, &evaluated_at) {
                    (Some(before), Some(now)) => now != before,
                    (None, Some(_)) => true,
                    _ => false,
                };
                if advanced {
                    s.observe_retries_left = 0;
                    s.stamp_before_save = None;
                } else {
                    s.observe_retries_left -= 1;
                    needs_repoll = s.observe_retries_left > 0;
                }
            }
            s.preview.rows = ui_rows;
            s.preview.songs = rows;
            s.preview.total = total;
            s.preview.evaluated_at = evaluated_at;
            s.preview.phase = Some(PreviewPhase::Loaded);
            s.preview.cursor = s.preview.cursor.min(s.preview.rows.len().saturating_sub(1));
        });
        // Artwork prefetch for the preview rows (fetch-by-album-id — the
        // rows render blank gray placeholders without it).
        let artwork = self.rules_preview_artwork_tasks();
        if needs_repoll {
            let delayed = Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_secs(OBSERVE_RETRY_SECS)).await;
                },
                |()| Message::RulesEditor(RulesEditorMessage::ReEvaluate),
            );
            Task::batch([artwork, delayed])
        } else {
            artwork
        }
    }

    // =====================================================================
    // Save flows (M4: direct to target — the M5 draft engine replaces the
    // create path's mechanics but not these entry points)
    // =====================================================================

    fn handle_rules_save(&mut self, save_as_new: bool) -> Task<Message> {
        // A mouse click on Save/Save-as-new lands with the just-typed value
        // still uncommitted in the editing buffer — flush it first or we save
        // the pre-edit rules and clear dirty (silent data loss).
        self.commit_pending_edit();
        self.revalidate_rules_session();
        let Some(session) = self.rules_session() else {
            return Task::none();
        };
        if session.saving {
            return Task::none();
        }
        if session.has_blocking_errors() {
            self.toast_warn("Fix the marked rule to save");
            return Task::none();
        }
        let rules_value = session.rules.to_value();
        let target = session.target.clone();
        let Some(editor) = self.playlist_editor.as_ref() else {
            return Task::none();
        };
        let name = editor.edit.playlist_name.trim().to_string();
        let comment = editor.edit.playlist_comment.clone();
        let public = editor.edit.playlist_public;
        let loaded_updated_at = editor.edit.loaded_updated_at().to_string();

        let draft = self.rules_session().and_then(|s| s.draft.clone());
        self.with_rules_session(|s| {
            s.saving = true;
            s.stamp_before_save = s.preview.evaluated_at.clone();
        });

        match (&target, save_as_new) {
            // Create — and the "Save as new…" fork from ANY session.
            (RulesTarget::Create, _) | (RulesTarget::Edit { .. }, true) => {
                // A fork FROM an edit session: the copy is a separate
                // playlist and the original session stays put (SaveCompleted
                // reads this to leave target/token/draft alone).
                let spun_off = save_as_new && !matches!(target, RulesTarget::Create);
                let create_name = if spun_off {
                    format!("{name} (copy)")
                } else {
                    name.clone()
                };
                let toast_name = create_name.clone();
                // With a live draft, the finalize is ONE atomic PUT: the
                // draft becomes the real playlist (name + user comment set,
                // marker cleared in the SAME body — never a two-step
                // clear). A crash right after leaves a finished,
                // unmarker'd playlist: safe. Draftless saves (never
                // previewed) POST directly — no marker is ever minted.
                // The draft finalizes into the playlist only on a create /
                // create-finalize — a spin-off leaves the original's draft
                // untouched (it belongs to the continuing session) and POSTs
                // a fresh copy.
                let fork_draft = (!spun_off).then_some(draft.clone()).flatten();
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        let id = match fork_draft {
                            Some(d) => {
                                service
                                    .put_playlist_full(
                                        &d.id,
                                        &create_name,
                                        &comment,
                                        public,
                                        Some(&rules_value),
                                        None,
                                    )
                                    .await?;
                                d.id
                            }
                            None => {
                                service
                                    .create_smart_playlist(
                                        &create_name,
                                        &comment,
                                        public,
                                        &rules_value,
                                    )
                                    .await?
                            }
                        };
                        let meta = service.get_playlist(&id).await?;
                        Ok((id, meta.updated_at))
                    },
                    move |result: Result<(String, String), anyhow::Error>| match result {
                        Ok((playlist_id, saved_updated_at)) => {
                            Message::RulesEditor(RulesEditorMessage::SaveCompleted {
                                playlist_id,
                                name: toast_name.clone(),
                                saved_updated_at,
                                detached: false,
                                spun_off,
                            })
                        }
                        Err(e) => map_save_error(e),
                    },
                )
            }
            (
                RulesTarget::Edit {
                    playlist_id,
                    file_backed,
                    sync,
                    ..
                },
                false,
            ) => {
                let playlist_id = playlist_id.clone();
                // File-backed detach on save: a still-synced file would
                // overwrite these rules on the next scan. 0.62+ only (the
                // PUT ignores `sync` on 0.61 — verified).
                let sync_detach =
                    (*file_backed && *sync && self.caps_state.caps().sync_via_put).then_some(false);
                let toast_name = name.clone();
                let detached = sync_detach.is_some();
                let id_for_msg = playlist_id.clone();
                let draft_for_task = draft.clone();
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        // Optimistic-concurrency check (the Tracks lane's
                        // exact pattern) — with the target-gone lane split
                        // from the conflict lane.
                        match service.get_playlist_updated_at(&playlist_id).await {
                            Ok(current) => {
                                if !loaded_updated_at.is_empty()
                                    && !current.is_empty()
                                    && current != loaded_updated_at
                                {
                                    return Ok(SaveOutcome::Conflict);
                                }
                            }
                            Err(e) if format!("{e:#}").contains("status 404") => {
                                return Ok(SaveOutcome::TargetGone);
                            }
                            Err(e) => return Err(e),
                        }
                        service
                            .put_playlist_full(
                                &playlist_id,
                                &name,
                                &comment,
                                public,
                                Some(&rules_value),
                                sync_detach,
                            )
                            .await?;
                        // The draft served its purpose — best-effort
                        // cleanup inside the same task (the sweep is the
                        // backstop). The UI keeps displaying the DRAFT's
                        // last evaluation: the real playlist's own
                        // evaluatedAt stays stale until its owner's next
                        // first-page read (documented cosmetic caveat).
                        if let Some(d) = draft_for_task {
                            let _ = service.delete_playlist(&d.id).await;
                        }
                        let saved = service
                            .get_playlist_updated_at(&playlist_id)
                            .await
                            .unwrap_or_default();
                        Ok(SaveOutcome::Saved { saved })
                    },
                    move |result: Result<SaveOutcome, anyhow::Error>| match result {
                        Ok(SaveOutcome::Saved { saved }) => {
                            Message::RulesEditor(RulesEditorMessage::SaveCompleted {
                                playlist_id: id_for_msg.clone(),
                                name: toast_name.clone(),
                                saved_updated_at: saved,
                                detached,
                                spun_off: false,
                            })
                        }
                        Ok(SaveOutcome::Conflict) => {
                            Message::RulesEditor(RulesEditorMessage::SaveConflict)
                        }
                        Ok(SaveOutcome::TargetGone) => {
                            Message::RulesEditor(RulesEditorMessage::SaveTargetGone)
                        }
                        Err(e) => map_save_error(e),
                    },
                )
            }
        }
    }

    /// ConfirmOverwrite: re-save skipping the concurrency check (the user
    /// chose to clobber).
    fn handle_rules_save_skip_check(&mut self) -> Task<Message> {
        if let Some(editor) = self.playlist_editor.as_mut() {
            // Blank the token so the next save's check passes vacuously.
            editor.edit.set_loaded_updated_at(String::new());
        }
        self.handle_rules_save(false)
    }

    /// ReloadServerRules: resolve a conflict by adopting the server's
    /// current rules + metadata.
    /// The in-session .nsp import resolved: load the file's criteria into
    /// the OPEN session like a disk-backed preset — edit-bar metadata from
    /// the envelope, rules replacing the (empty) tree, then the same
    /// preset-style draft preview dispatch. No server write happens here;
    /// Save owns that later.
    fn handle_nsp_parsed_into_session(
        &mut self,
        result: crate::update::nsp_import::NspPickResult,
    ) -> Task<Message> {
        use crate::update::nsp_import::NspPickResult;
        let payload = match result {
            NspPickResult::Cancelled => return Task::none(),
            NspPickResult::Failed(reason) => {
                self.toast_error(format!("Not a valid smart-playlist file — {reason}"));
                return Task::none();
            }
            NspPickResult::Parsed(payload) => payload,
        };
        if let Some(editor) = self.playlist_editor.as_mut() {
            editor.edit.playlist_name = payload.name.clone();
            editor.edit.playlist_comment = payload.comment.clone();
            editor.edit.playlist_public = payload.public;
        }
        self.with_rules_session_revalidate(|s| {
            s.rules = SmartRules::parse(&payload.rules);
            s.dirty = true;
            s.mode = FormMode::Cursor;
            s.rebuild_rows();
            s.cursor = s
                .rows
                .iter()
                .position(|r| matches!(r, FormRow::Match))
                .unwrap_or(0);
            s.normalize_cell();
        });
        // Imported rules are real rules — run the open-POST/preview now,
        // exactly like a preset-seeded session.
        self.dispatch_draft_preview()
    }

    fn handle_reload_server_rules(&mut self) -> Task<Message> {
        let Some(playlist_id) = self.rules_session().and_then(|s| match &s.target {
            RulesTarget::Edit { playlist_id, .. } => Some(playlist_id.clone()),
            RulesTarget::Create => None,
        }) else {
            return Task::none();
        };
        self.with_rules_session(|s| s.save_conflict = false);
        self.shell_task(
            move |shell| async move {
                let service = shell.playlists_api().await?;
                let meta = service.get_playlist(&playlist_id).await?;
                Ok(Box::new(meta))
            },
            |result: Result<Box<Playlist>, anyhow::Error>| match result {
                Ok(meta) => Message::RulesEditor(RulesEditorMessage::PlaylistKindResolved {
                    playlist_id: meta.id.clone(),
                    result: Ok(meta),
                }),
                Err(e) => Message::Toast(crate::app_message::ToastMessage::Push(
                    nokkvi_data::types::toast::Toast::new(
                        format!("Failed to reload rules: {e}"),
                        nokkvi_data::types::toast::ToastLevel::Error,
                    ),
                )),
            },
        )
    }

    // =====================================================================
    // Pencil JIT resolution (the M1 refusal's M4 replacement)
    // =====================================================================

    /// Kick off the banner pencil's just-in-time meta fetch for an
    /// unknown-smartness active playlist.
    pub(crate) fn dispatch_playlist_kind_fetch(&mut self, playlist_id: String) -> Task<Message> {
        let id_for_msg = playlist_id.clone();
        self.shell_task(
            move |shell| async move {
                let service = shell.playlists_api().await?;
                let meta = service.get_playlist(&playlist_id).await?;
                Ok(Box::new(meta))
            },
            move |result: Result<Box<Playlist>, anyhow::Error>| match result {
                Ok(meta) => Message::RulesEditor(RulesEditorMessage::PlaylistKindResolved {
                    playlist_id: id_for_msg.clone(),
                    result: Ok(meta),
                }),
                Err(e) => Message::RulesEditor(RulesEditorMessage::PlaylistKindResolved {
                    playlist_id: id_for_msg.clone(),
                    result: Err(format!("{e:#}")),
                }),
            },
        )
    }

    fn handle_playlist_kind_resolved(
        &mut self,
        playlist_id: String,
        result: Result<Box<Playlist>, String>,
    ) -> Task<Message> {
        match result {
            Err(e) => {
                warn!("playlist kind fetch failed: {e}");
                self.toast_warn("Couldn't reach the server — open it from the Playlists view");
                Task::none()
            }
            Ok(meta) => {
                // A conflict-reload lands here too: refresh an OPEN rules
                // session for the same playlist in place.
                let session_matches = self.rules_session().is_some_and(|s| {
                    matches!(&s.target, RulesTarget::Edit { playlist_id: id, .. } if *id == playlist_id)
                });
                if session_matches {
                    let rules = meta
                        .rules
                        .as_ref()
                        .map(SmartRules::parse)
                        .unwrap_or_default();
                    if let Some(editor) = self.playlist_editor.as_mut() {
                        editor.edit.playlist_name = meta.name.clone();
                        editor.edit.playlist_comment = meta.comment.clone();
                        editor.edit.playlist_public = meta.public;
                        editor.edit.set_loaded_updated_at(meta.updated_at.clone());
                    }
                    self.with_rules_session_revalidate(|s| {
                        s.rules = rules;
                        s.dirty = false;
                        s.rebuild_rows();
                        s.preview.evaluated_at = meta.evaluated_at.clone();
                    });
                    self.toast_info("Reloaded the server's rules");
                    return Task::none();
                }
                if meta.is_smart() {
                    // Session-from-meta: the JIT-fetched row may not be in
                    // the library list (ownership + caps re-checked inside).
                    self.enter_rules_mode_from_meta(&meta)
                } else {
                    Task::done(Message::SplitView(
                        crate::app_message::SplitViewMessage::EnterEditMode {
                            playlist_id: meta.id.clone(),
                            playlist_name: meta.name.clone(),
                            playlist_comment: meta.comment.clone(),
                            playlist_public: meta.public,
                        },
                    ))
                }
            }
        }
    }

    // =====================================================================
    // Small shared plumbing
    // =====================================================================

    pub(crate) fn rules_session(&self) -> Option<&RulesSessionUi> {
        self.playlist_editor
            .as_ref()
            .and_then(|e| e.rules_session())
    }

    pub(crate) fn with_rules_session(&mut self, f: impl FnOnce(&mut RulesSessionUi)) {
        if let Some(session) = self
            .playlist_editor
            .as_mut()
            .and_then(|e| e.rules_session_mut())
        {
            f(session);
        }
    }

    fn with_rules_session_revalidate(&mut self, f: impl FnOnce(&mut RulesSessionUi)) {
        self.with_rules_session(f);
        self.revalidate_rules_session();
    }

    /// Immediate validation (like search) against the current edit-bar
    /// name + session context.
    pub(crate) fn revalidate_rules_session(&mut self) {
        let Some(editor) = self.playlist_editor.as_mut() else {
            return;
        };
        let name = editor.edit.playlist_name.clone();
        let library_ids: Vec<i32> = self
            .app_service
            .as_ref()
            .map(|s| s.active_library_ids_vec())
            .unwrap_or_default();
        if let Some(session) = editor.rules_session_mut() {
            let target_id = match &session.target {
                RulesTarget::Edit { playlist_id, .. } => Some(playlist_id.clone()),
                RulesTarget::Create => None,
            };
            session.revalidate(&name, target_id.as_deref(), &library_ids);
        }
    }
}

/// The save task's tri-state outcome.
enum SaveOutcome {
    Saved { saved: String },
    Conflict,
    TargetGone,
}

fn map_save_error(e: anyhow::Error) -> Message {
    if let Some(msg) = crate::update::components::session_expired_message(&e) {
        return msg;
    }
    Message::RulesEditor(RulesEditorMessage::SaveFailed(format!("{e:#}")))
}

/// Build a read-only preview row from a Song (the editor row renderer's
/// input type). `entry_id` = the row index — the pane never mutates.
fn song_to_preview_row(
    song: &nokkvi_data::types::song::Song,
    index: usize,
) -> nokkvi_data::backend::queue::QueueSongUIViewData {
    let duration_secs = song.duration;
    nokkvi_data::backend::queue::QueueSongUIViewData {
        id: song.id.clone(),
        entry_id: index as u64,
        track_number: (index + 1) as i32,
        title: song.title.clone(),
        artist: song.artist.clone(),
        artist_id: song.artist_id.clone().unwrap_or_default(),
        album: song.album.clone(),
        album_id: song.album_id.clone().unwrap_or_default(),
        artwork_url: String::new(),
        updated_at: song.updated_at.clone(),
        duration: nokkvi_data::utils::formatters::format_time(duration_secs),
        duration_seconds: duration_secs,
        genre: song.genre.clone().unwrap_or_default(),
        starred: song.starred,
        rating: song.rating,
        play_count: song.play_count,
        searchable_lower: nokkvi_data::utils::search::build_searchable_lower(&[
            &song.title,
            &song.artist,
            &song.album,
        ]),
    }
}

fn field_label(
    registry: &nokkvi_data::types::smart_criteria::FieldRegistry,
    name: &str,
) -> Option<String> {
    match registry.lookup(name)? {
        FieldKind::Column(def) => Some(def.label.to_owned()),
        FieldKind::Role => Some(format!("{name} (role)")),
        FieldKind::Tag => Some(name.to_owned()),
    }
}

/// Cycle a `&str` through a static slice (wrapping; absent ⇒ first).
fn cycle_slice<'a>(items: &[&'a str], current: &str, forward: bool) -> &'a str {
    let idx = items.iter().position(|i| *i == current);
    match idx {
        None => items.first().copied().unwrap_or(""),
        Some(i) => {
            let len = items.len();
            let next = if forward {
                (i + 1) % len
            } else {
                (i + len - 1) % len
            };
            items[next]
        }
    }
}

/// Cycle within an owned list of operators.
fn cycle_items(
    items: &[RuleOperator],
    current: &RuleOperator,
    forward: bool,
) -> Option<RuleOperator> {
    if items.is_empty() {
        return None;
    }
    let idx = items.iter().position(|i| i == current);
    Some(match idx {
        None => items[0],
        Some(i) => {
            let len = items.len();
            let next = if forward {
                (i + 1) % len
            } else {
                (i + len - 1) % len
            };
            items[next]
        }
    })
}

/// Quick-cycle sort fields (the corpus's sort favorites).
const SORT_QUICK_FIELDS: &[&str] = &[
    "playcount",
    "dateadded",
    "dateloved",
    "lastplayed",
    "rating",
    "daterated",
    "title",
    "album",
    "year",
    "duration",
    "random",
];

// =========================================================================
// RulesSessionUi mutation helpers used by the handler (kept here so the
// state file stays presentation-free; they operate purely on the model)
// =========================================================================

impl RulesSessionUi {
    /// The value shape of the leaf at `path` (None for non-leaves).
    pub(crate) fn value_shape_at(&self, path: &[usize]) -> Option<ValueShape> {
        match self.node_at(path)? {
            CriteriaNode::Leaf(leaf) => {
                Some(leaf.operator.value_shape(self.field_class_of(&leaf.field)))
            }
            _ => None,
        }
    }

    /// The operators the picker offers for the leaf's field — invalid
    /// combinations are never offered (multi-value fields drop ranges;
    /// presence ops appear only where valid under the caps).
    pub(crate) fn valid_operators_for_row(&self, path: &[usize]) -> Vec<RuleOperator> {
        let field = match self.node_at(path) {
            Some(CriteriaNode::Leaf(leaf)) => leaf.field.clone(),
            _ => return RuleOperator::ALL.to_vec(),
        };
        let kind = self.registry.lookup(&field);
        RuleOperator::ALL
            .into_iter()
            .filter(|op| match op {
                RuleOperator::InTheRange => !kind
                    .is_some_and(nokkvi_data::types::smart_criteria::FieldKind::is_multi_valued),
                RuleOperator::IsMissing | RuleOperator::IsPresent => {
                    self.registry.presence_ops_valid(&field, &self.caps)
                }
                RuleOperator::InPlaylist | RuleOperator::NotInPlaylist => true,
                _ => true,
            })
            .collect()
    }

    /// The display/edit text of a leaf's value slot.
    pub(crate) fn leaf_value_text(&self, path: &[usize], slot2: bool) -> String {
        let Some(CriteriaNode::Leaf(leaf)) = self.node_at(path) else {
            return String::new();
        };
        let value = if let Some(arr) = leaf.value.as_array() {
            arr.get(usize::from(slot2)).cloned().unwrap_or_default()
        } else {
            leaf.value.clone()
        };
        match value {
            serde_json::Value::String(s) => s,
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        }
    }

    /// Commit edited text into a leaf's value slot, typed by its shape.
    pub(crate) fn set_leaf_value_text(&mut self, path: &[usize], text: &str, slot2: bool) {
        let shape = self.value_shape_at(path);
        let Some(CriteriaNode::Leaf(leaf)) = self.node_at_mut(path) else {
            return;
        };
        let typed = type_value(text, shape);
        match shape {
            Some(ValueShape::Pair | ValueShape::DatePair) => {
                let mut arr = leaf
                    .value
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![serde_json::Value::Null, serde_json::Value::Null]);
                while arr.len() < 2 {
                    arr.push(serde_json::Value::Null);
                }
                arr[usize::from(slot2)] = typed;
                leaf.value = serde_json::Value::Array(arr);
            }
            _ => leaf.value = typed,
        }
        self.dirty = true;
    }

    /// Toggle a boolean leaf value (On/Off pills).
    pub(crate) fn toggle_leaf_bool(&mut self, path: &[usize]) {
        if let Some(CriteriaNode::Leaf(leaf)) = self.node_at_mut(path) {
            let current = leaf.value.as_bool().unwrap_or(false);
            leaf.value = serde_json::Value::Bool(!current);
            self.dirty = true;
        }
    }

    /// Set a leaf's field, reshaping the value when the shape flips.
    pub(crate) fn set_leaf_field(&mut self, path: &[usize], field: &str) {
        let before = self.value_shape_at(path);
        let class_after = self.field_class_of(field);
        if let Some(CriteriaNode::Leaf(leaf)) = self.node_at_mut(path) {
            leaf.field = field.to_owned();
            let after = leaf.operator.value_shape(class_after);
            if before != Some(after) {
                leaf.value = default_value_for(after);
            }
            self.dirty = true;
        }
        // The new field may not accept the surviving operator — e.g. cycling a
        // range rule onto a multi-valued tag, which drops `inTheRange` from the
        // valid set. Snap to a valid operator so the value cell never renders a
        // two-input range editor over a tag. set_leaf_operator reshapes the
        // value + rebuilds, so return before the fall-through rebuild.
        let current_op = match self.node_at(path) {
            Some(CriteriaNode::Leaf(leaf)) => Some(leaf.operator),
            _ => None,
        };
        if let Some(op) = current_op {
            let valid = self.valid_operators_for_row(path);
            if !valid.contains(&op)
                && let Some(&fallback) = valid.first()
            {
                self.set_leaf_operator(path, fallback);
                return;
            }
        }
        self.rebuild_rows();
    }

    /// Set a leaf's operator (canonical spelling), reshaping the value
    /// when the shape flips.
    pub(crate) fn set_leaf_operator(&mut self, path: &[usize], op: RuleOperator) {
        let class = match self.node_at(path) {
            Some(CriteriaNode::Leaf(leaf)) => self.field_class_of(&leaf.field),
            _ => FieldClass::Text,
        };
        let before = self.value_shape_at(path);
        if let Some(CriteriaNode::Leaf(leaf)) = self.node_at_mut(path) {
            leaf.operator = op;
            leaf.original_key = op.wire_key().to_owned();
            let after = op.value_shape(class);
            if before != Some(after) {
                leaf.value = default_value_for(after);
            }
            // Presence ops carry the field-flag operand shape.
            if matches!(op, RuleOperator::IsMissing | RuleOperator::IsPresent) {
                leaf.value = serde_json::Value::Bool(true);
            }
            self.dirty = true;
        }
        self.rebuild_rows();
    }
}

fn type_value(text: &str, shape: Option<ValueShape>) -> serde_json::Value {
    let trimmed = text.trim();
    match shape {
        Some(ValueShape::Number) => trimmed
            .parse::<i64>()
            .map(serde_json::Value::from)
            .or_else(|_| trimmed.parse::<f64>().map(serde_json::Value::from))
            .unwrap_or_else(|_| serde_json::Value::String(trimmed.to_owned())),
        Some(ValueShape::Days) => trimmed.parse::<i64>().map_or_else(
            |_| serde_json::Value::String(trimmed.to_owned()),
            serde_json::Value::from,
        ),
        Some(ValueShape::Toggle) => serde_json::Value::Bool(trimmed.eq_ignore_ascii_case("true")),
        Some(ValueShape::Pair) => trimmed.parse::<i64>().map_or_else(
            |_| serde_json::Value::String(trimmed.to_owned()),
            serde_json::Value::from,
        ),
        // Dates, text, playlist refs, date-pairs: strings.
        _ => serde_json::Value::String(trimmed.to_owned()),
    }
}

fn default_value_for(shape: ValueShape) -> serde_json::Value {
    match shape {
        ValueShape::Number => serde_json::json!(0),
        ValueShape::Days => serde_json::json!(30),
        ValueShape::Toggle => serde_json::json!(true),
        ValueShape::Pair => serde_json::json!([0, 0]),
        ValueShape::DatePair => serde_json::json!(["", ""]),
        ValueShape::FieldFlag => serde_json::json!(true),
        ValueShape::Text | ValueShape::Date | ValueShape::PlaylistRef => serde_json::json!(""),
    }
}
