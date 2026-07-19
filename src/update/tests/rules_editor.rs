//! M4 rules-session handler tests — the three-mode focus machine, row
//! lifecycle, sub-picker ownership, JSON mode, interim preview semantics,
//! and the save lanes. Driven through the full dispatcher where possible;
//! observable state only.

use nokkvi_data::types::{
    rules_session::RulesTarget,
    smart_criteria::{CriteriaNode, ServerCaps},
    toast::ToastLevel,
};

use crate::{
    Message,
    app_message::{RulesEditorMessage as R, RulesEntryTarget, SplitViewMessage},
    state::{CapsState, FormCell, FormMode, FormRow, RulesPane},
    test_helpers::test_app,
};

fn smart_row(
    id: &str,
    name: &str,
    owner_id: &str,
) -> nokkvi_data::backend::playlists::PlaylistUIViewData {
    nokkvi_data::backend::playlists::PlaylistUIViewData {
        id: id.into(),
        name: name.into(),
        comment: String::new(),
        duration: 0.0,
        song_count: 3,
        owner_name: "foogs".into(),
        public: true,
        updated_at: "T0".into(),
        artwork_album_ids: vec![],
        uploaded_image: None,
        is_smart: true,
        rules: Some(serde_json::json!({
            "all": [ { "is": { "loved": true } } ],
            "sort": "dateloved",
            "limit": 100
        })),
        evaluated_at: Some("2026-07-01T10:00:00Z".into()),
        is_file_backed: false,
        sync: false,
        owner_id: owner_id.into(),
        searchable_lower: name.to_lowercase(),
    }
}

/// A capable app: caps fetched (0.63.2), session user id known, and OFF
/// the login screen (whose guard would short-circuit `handle_raw_key_event`
/// and make every keyboard test pass vacuously).
fn capable_app() -> crate::Nokkvi {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.caps_state = CapsState::Fetched(ServerCaps::from_version_str("0.63.2"));
    app.session_user_id = "user-9".into();
    app
}

fn open_create(app: &mut crate::Nokkvi) {
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterRulesMode {
        target: RulesEntryTarget::Create,
    }));
}

fn open_edit(app: &mut crate::Nokkvi) {
    app.library
        .playlists
        .append_page(vec![smart_row("sp1", "Loved", "user-9")], 1);
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterRulesMode {
        target: RulesEntryTarget::Edit {
            playlist_id: "sp1".into(),
        },
    }));
}

fn session(app: &crate::Nokkvi) -> &crate::state::RulesSessionUi {
    app.rules_session().expect("rules session mounted")
}

// --- Session open ----------------------------------------------------------

/// EnterRulesMode(Create): Rules kind mounts in View::PlaylistEditor with
/// NO browsing panel, a placeholder (empty, private) identity, and focus
/// seeded in EDITING mode on the name input — the one-screen create flow.
/// No draft exists (the M5 blank-create rule holds trivially in M4).
#[test]
fn enter_create_mounts_rules_session_editing_name() {
    let mut app = capable_app();
    open_create(&mut app);

    assert_eq!(app.current_view, crate::View::PlaylistEditor);
    assert!(app.browsing_panel.is_none(), "rules mode has no browser");
    let editor = app.playlist_editor.as_ref().expect("editor mounted");
    assert!(editor.edit.playlist_name.is_empty(), "placeholder name");
    assert!(!editor.edit.playlist_public, "creates default private");
    let s = session(&app);
    assert!(matches!(s.target, RulesTarget::Create));
    assert_eq!(s.mode, FormMode::Editing);
    assert_eq!(s.cell, FormCell::Name);
    assert!(s.draft.is_none(), "no draft object in M4");
}

/// EnterRulesMode(Edit): seed focus = CURSOR mode on the match row (the
/// rules are the object of the visit), rules parsed from the row's
/// substrate, evaluatedAt carried into the pane.
#[test]
fn enter_edit_seeds_cursor_on_match_row() {
    let mut app = capable_app();
    open_edit(&mut app);

    let s = session(&app);
    assert!(matches!(s.target, RulesTarget::Edit { .. }));
    assert_eq!(s.mode, FormMode::Cursor);
    assert!(matches!(s.rows[s.cursor], FormRow::Match));
    assert_eq!(
        s.preview.evaluated_at.as_deref(),
        Some("2026-07-01T10:00:00Z")
    );
}

/// The preview columns cog flips a column's visibility on the PERSISTENT
/// `Nokkvi.preview_column_visibility` (survives editor close/reopen, unlike
/// the ephemeral session), per-column independent. All five default ON, so the
/// first toggle turns one OFF; a second restores it.
#[test]
fn toggle_preview_column_flips_persistent_visibility() {
    use crate::state::PreviewColumn;
    let mut app = capable_app();
    open_create(&mut app);
    assert!(app.preview_column_visibility.stars, "stars default on");

    let _ = app.update(Message::RulesEditor(R::ToggleColumnVisible(
        PreviewColumn::Stars,
    )));
    assert!(
        !app.preview_column_visibility.stars,
        "first toggle turns off"
    );

    let _ = app.update(Message::RulesEditor(R::ToggleColumnVisible(
        PreviewColumn::Stars,
    )));
    assert!(
        app.preview_column_visibility.stars,
        "second toggle restores"
    );

    // A different column toggles independently.
    let _ = app.update(Message::RulesEditor(R::ToggleColumnVisible(
        PreviewColumn::Genre,
    )));
    assert!(!app.preview_column_visibility.genre, "genre toggled off");
    assert!(app.preview_column_visibility.stars, "stars unaffected");
}

/// The caps gate: with the capability unknown, no session mounts and the
/// refusal toast fires (conservative = feature-hidden).
#[test]
fn enter_rules_mode_refuses_without_caps() {
    let mut app = test_app(); // caps Unfetched
    app.session_user_id = "user-9".into();
    open_create(&mut app);

    assert!(app.playlist_editor.is_none());
    assert_eq!(
        app.toast.toasts.back().map(|t| t.level),
        Some(ToastLevel::Warning)
    );
}

/// The ownership gate: an unowned smart row never opens a session whose
/// Save would always 403.
#[test]
fn enter_edit_refuses_unowned() {
    let mut app = capable_app();
    app.library
        .playlists
        .append_page(vec![smart_row("sp2", "Theirs", "user-OTHER")], 1);
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterRulesMode {
        target: RulesEntryTarget::Edit {
            playlist_id: "sp2".into(),
        },
    }));

    assert!(app.playlist_editor.is_none());
    let toast = app.toast.toasts.back().expect("refusal toast");
    assert!(toast.message.contains("owner"), "got: {}", toast.message);
}

// --- Focus machine ---------------------------------------------------------

/// Enter on a value cell enters Editing; Commit writes the typed value and
/// returns to cursor; Escape-in-Editing reverts the CELL only (the session
/// survives with its dirty state intact).
#[test]
fn editing_mode_commit_and_revert() {
    let mut app = capable_app();
    open_edit(&mut app);

    // Move the cursor onto the rule row's value cell.
    app.with_rules_session(|s| {
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("a rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    // `loved is <bool>` renders Toggle — Enter flips instead of editing.
    // Retarget: switch the leaf's operator cell instead for a text edit —
    // use the limit row (a real input cell).
    app.with_rules_session(|s| {
        s.mode = FormMode::Cursor;
        s.editing = None;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Limit))
            .expect("limit row");
        s.cell = FormCell::LimitValue;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    assert_eq!(
        session(&app).mode,
        FormMode::Editing,
        "input cell → Editing"
    );

    let _ = app.update(Message::RulesEditor(R::EditingInput("250".into())));
    let _ = app.update(Message::RulesEditor(R::CommitEditing));
    let s = session(&app);
    assert_eq!(s.mode, FormMode::Cursor, "commit returns to cursor mode");
    assert_eq!(s.rules.limit, Some(250), "the typed value landed");

    // Revert lane: open again, type, Escape-revert — value unchanged.
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let _ = app.update(Message::RulesEditor(R::EditingInput("7".into())));
    let _ = app.update(Message::RulesEditor(R::RevertEditing));
    let s = session(&app);
    assert_eq!(s.mode, FormMode::Cursor);
    assert_eq!(s.rules.limit, Some(250), "Escape reverts the CELL only");
    assert!(
        app.playlist_editor.is_some(),
        "Escape in Editing NEVER discards the session"
    );
}

/// Escape in cursor mode with a dirty form surfaces the discard confirm;
/// CancelDiscard keeps the session; ConfirmDiscard exits.
#[test]
fn cursor_escape_discard_confirm_flow() {
    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| s.dirty = true);

    let _ = app.update(Message::RulesEditor(R::EscapePressed));
    assert!(session(&app).confirm_discard, "dirty Escape → confirm");

    let _ = app.update(Message::RulesEditor(R::CancelDiscard));
    assert!(!session(&app).confirm_discard);
    assert!(app.playlist_editor.is_some());

    let _ = app.update(Message::RulesEditor(R::EscapePressed));
    let _ = app.update(Message::RulesEditor(R::ConfirmDiscard));
    assert!(
        app.playlist_editor.is_none(),
        "confirm discards the session"
    );
}

// --- Row lifecycle (the keyboard-complete authoring loop) ------------------

/// Enter on the trailing add-row appends a rule and moves the cursor onto
/// it; Delete removes it. The field sub-picker opens from the field cell,
/// navigates, and commits the field.
#[test]
fn row_lifecycle_add_pick_remove() {
    let mut app = capable_app();
    open_edit(&mut app);

    // Add via the ROOT trailing add-row.
    app.with_rules_session(|s| {
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::AddRule(p) if p.is_empty()))
            .expect("root add-row");
        s.cell = FormCell::RowAction;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let s = session(&app);
    assert!(
        matches!(&s.rows[s.cursor], FormRow::Rule(p) if *p == vec![1]),
        "cursor lands on the appended rule"
    );
    assert_eq!(s.cell, FormCell::Field);

    // Field sub-picker: open on the field cell, filter, commit.
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    assert!(session(&app).sub_picker.is_some(), "field picker opened");
    let _ = app.update(Message::RulesEditor(R::SubPickerQuery("playcount".into())));
    let _ = app.update(Message::RulesEditor(R::SubPickerCommit));
    let s = session(&app);
    assert!(s.sub_picker.is_none());
    let Some(CriteriaNode::Leaf(leaf)) = s.node_at(&[1]) else {
        panic!("leaf expected");
    };
    assert_eq!(leaf.field, "playcount", "the picked field landed");

    // Delete the row.
    let _ = app.update(Message::RulesEditor(R::DeleteCursorRow));
    let s = session(&app);
    assert_eq!(
        s.rules.root.as_ref().map(|r| r.nodes.len()),
        Some(1),
        "Delete removed the cursor rule"
    );
}

/// With a sub-picker open, a resolved non-nav hotkey (t → OpenTrawl) is
/// SWALLOWED — never fired against the obscured form (the outer-gate rule,
/// mirrored from the default_playlist_picker precedent).
#[test]
fn sub_picker_swallows_non_nav_hotkeys() {
    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.sub_picker = Some(crate::state::SubPicker {
            kind: crate::state::SubPickerKind::Field {
                row: FormRow::Rule(vec![0]),
            },
            query: String::new(),
            cursor: 0,
        });
    });

    let _ = app.handle_raw_key_event(
        iced::keyboard::Key::Character("t".into()),
        iced::keyboard::Modifiers::default(),
        iced::event::Status::Ignored,
    );

    assert!(
        app.trawl_modal.is_none(),
        "t must not open Trawl under the rules sub-picker"
    );
    assert!(
        session(&app).sub_picker.is_some(),
        "the picker stays open (the key was swallowed, not routed)"
    );
}

// --- JSON mode -------------------------------------------------------------

/// Enter on the JSON toggle row enters JSON mode with the snapshot taken;
/// a clean-parse Escape applies the parsed rules; a parse error offers the
/// revert lane, and revert restores the snapshot.
#[test]
fn json_mode_entry_apply_and_revert() {
    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::JsonToggle))
            .expect("json row");
        s.cell = FormCell::RowAction;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let s = session(&app);
    assert_eq!(
        s.mode,
        FormMode::Json,
        "Enter on the JSON row enters JSON mode"
    );
    assert!(s.json.is_some(), "snapshot + editor content taken");

    // Clean parse applies.
    let limit_before = session(&app).rules.limit;
    let _ = app.update(Message::RulesEditor(R::JsonEscape));
    let s = session(&app);
    assert_eq!(s.mode, FormMode::Cursor, "clean Escape exits JSON mode");
    assert_eq!(s.rules.limit, limit_before, "no-edit apply is lossless");

    // Parse error pins the mode + offers revert; revert restores.
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    app.with_rules_session(|s| {
        if let Some(json) = s.json.as_mut() {
            json.content = iced::widget::text_editor::Content::with_text("{ not json");
        }
    });
    let _ = app.update(Message::RulesEditor(R::JsonEscape));
    let s = session(&app);
    assert_eq!(s.mode, FormMode::Json, "a parse error pins JSON mode");
    assert!(s.json.as_ref().is_some_and(|j| j.revert_offer));

    let _ = app.update(Message::RulesEditor(R::JsonRevertToLastGood));
    let s = session(&app);
    assert_eq!(s.mode, FormMode::Cursor);
    assert_eq!(s.rules.limit, limit_before, "revert restored the snapshot");
}

// --- M4 interim preview semantics ------------------------------------------

/// A blank create dispatches ZERO draft network at open (the ruled pin),
/// and a Preview press on it is refused by validation — no draft POST can
/// ever carry an empty conjunction (structural: the serializer is
/// unreachable while the empty-root Error stands).
#[test]
fn blank_create_stays_draftless_and_preview_gated() {
    let mut app = capable_app();
    open_create(&mut app);

    let s = session(&app);
    assert!(s.draft.is_none(), "zero draft network at open");
    assert!(s.last_written_rules.is_none());
    assert!(
        s.preview.phase.is_none(),
        "nothing pretends an evaluation happened"
    );

    app.toast.toasts.clear();
    let _ = app.update(Message::RulesEditor(R::Preview));
    assert_eq!(
        app.toast.toasts.back().map(|t| t.level),
        Some(ToastLevel::Warning),
        "an empty root blocks Preview with the validation refusal"
    );
    assert!(
        session(&app).draft.is_none(),
        "still no draft after refusal"
    );
}

/// DraftPreviewLoaded establishes the draft + the written-rules baseline
/// and populates the pane; a STALE generation is dropped without touching
/// the session's draft.
#[test]
fn draft_preview_loaded_establishes_and_stale_drops() {
    let mut app = capable_app();
    open_edit(&mut app);
    let generation = app.rules_preview_generation;
    let written = serde_json::json!({ "all": [ { "is": { "loved": true } } ] });

    let _ = app.update(Message::RulesEditor(R::DraftPreviewLoaded {
        generation,
        draft: nokkvi_data::types::rules_session::DraftInfo {
            id: "draft-1".into(),
            marker: "nokkvi-draft/1 pid=1 ts=2".into(),
        },
        written_rules: written.clone(),
        rows: vec![],
        total: Some(0),
        evaluated_at: Some("2026-07-18T10:00:00Z".into()),
    }));
    let s = session(&app);
    assert_eq!(s.draft.as_ref().map(|d| d.id.as_str()), Some("draft-1"));
    assert_eq!(s.last_written_rules.as_ref(), Some(&written));
    assert_eq!(s.preview.source_id.as_deref(), Some("draft-1"));

    // Stale generation: dropped, draft untouched.
    let _ = app.update(Message::RulesEditor(R::DraftPreviewLoaded {
        generation: generation.wrapping_sub(1),
        draft: nokkvi_data::types::rules_session::DraftInfo {
            id: "draft-STALE".into(),
            marker: "nokkvi-draft/1 pid=1 ts=3".into(),
        },
        written_rules: written,
        rows: vec![],
        total: Some(0),
        evaluated_at: None,
    }));
    assert_eq!(
        session(&app).draft.as_ref().map(|d| d.id.as_str()),
        Some("draft-1"),
        "a stale write never replaces the live draft"
    );
}

/// A mid-session draft 404 recreates transparently ONCE (loop-guarded);
/// a second failure lands in the honest Failed phase with last-good rows
/// retained.
#[test]
fn preview_failed_404_recreates_once() {
    let mut app = capable_app();
    open_edit(&mut app);
    let generation = app.rules_preview_generation;
    app.with_rules_session(|s| {
        s.draft = Some(nokkvi_data::types::rules_session::DraftInfo {
            id: "draft-1".into(),
            marker: "m".into(),
        });
        s.last_written_rules = Some(serde_json::json!({"all": []}));
    });

    let _ = app.update(Message::RulesEditor(R::PreviewFailed {
        generation,
        error: "API GET failed with status 404: gone".into(),
    }));
    let s = session(&app);
    assert!(
        s.draft.is_none(),
        "the vanished draft was dropped for recreate"
    );
    assert!(s.draft_recreate_attempted, "the one-shot guard armed");

    let generation = app.rules_preview_generation;
    let _ = app.update(Message::RulesEditor(R::PreviewFailed {
        generation,
        error: "API GET failed with status 404: gone".into(),
    }));
    assert_eq!(
        session(&app).preview.phase(),
        crate::state::PreviewPhase::Failed,
        "a second failure is surfaced honestly, not looped"
    );
}

/// DraftUnavailable = authoring-only mode: the form stays usable, the pane
/// shows the unreachable copy (with Retry in the view).
#[test]
fn draft_unavailable_sets_authoring_only() {
    let mut app = capable_app();
    open_create(&mut app);
    let generation = app.rules_preview_generation;
    let _ = app.update(Message::RulesEditor(R::DraftUnavailable {
        generation,
        error: "connect refused".into(),
    }));
    assert_eq!(
        session(&app).preview.phase(),
        crate::state::PreviewPhase::Unavailable
    );
}

/// SaveCompleted with a live draft consumes it and SKIPS the observe loop
/// — the pane already shows the draft's evaluation (the saved truth).
#[test]
fn save_completed_with_draft_skips_observe() {
    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.draft = Some(nokkvi_data::types::rules_session::DraftInfo {
            id: "draft-1".into(),
            marker: "m".into(),
        });
    });

    let _ = app.update(Message::RulesEditor(R::SaveCompleted {
        playlist_id: "sp1".into(),
        name: "Loved".into(),
        saved_updated_at: "T2".into(),
        detached: false,
        spun_off: false,
    }));

    let s = session(&app);
    assert!(s.draft.is_none(), "the draft served its purpose");
    assert_eq!(
        s.observe_retries_left, 0,
        "draft-backed saves never re-poll the target"
    );
}

/// PreviewPageLoaded appends `_start > 0` rows of the same evaluation.
#[test]
fn preview_page_loaded_appends() {
    let mut app = capable_app();
    open_edit(&mut app);
    let generation = app.rules_preview_generation;
    app.with_rules_session(|s| {
        s.preview.page_loading = true;
        s.preview.total = Some(2);
    });
    let song = nokkvi_data::types::song::Song {
        id: "s-page".into(),
        title: "Paged Song".into(),
        ..Default::default()
    };
    let _ = app.update(Message::RulesEditor(R::PreviewPageLoaded {
        generation,
        rows: vec![song],
    }));
    let s = session(&app);
    assert_eq!(s.preview.rows.len(), 1);
    assert_eq!(s.preview.songs.len(), 1);
    assert!(!s.preview.page_loading);
}

/// Enter-to-play runs the guarded play path: an active radio transitions
/// back to queue mode (guard_play_action's observable side effect) before
/// the preloaded play dispatches.
#[test]
fn play_preview_row_consults_the_play_guard() {
    use crate::state::{ActivePlayback, RadioPlaybackState};
    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.preview.songs = vec![nokkvi_data::types::song::Song {
            id: "s1".into(),
            title: "Song One".into(),
            ..Default::default()
        }];
        s.preview.cursor = 0;
    });
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

    let _ = app.update(Message::RulesEditor(R::PlayPreviewRow));

    assert!(
        app.active_playback.is_queue(),
        "guard_play_action transitioned radio → queue before the play"
    );
}

// --- Save lanes ------------------------------------------------------------

/// Save with a blocking Error diagnostic refuses (warn toast, not saving).
#[test]
fn save_blocked_by_validation_errors() {
    let mut app = capable_app();
    open_create(&mut app);
    // Blank create: empty root + empty name = blocking Errors.
    let _ = app.update(Message::RulesEditor(R::Save));

    let s = session(&app);
    assert!(!s.saving, "a blocked save never goes in flight");
    assert_eq!(
        app.toast.toasts.back().map(|t| t.level),
        Some(ToastLevel::Warning)
    );
}

/// SaveConflict / SaveTargetGone set their distinct recovery states — the
/// two lanes are never conflated (a dead-end "reload" of a deleted
/// playlist must be impossible).
#[test]
fn save_conflict_and_target_gone_are_distinct_lanes() {
    let mut app = capable_app();
    open_edit(&mut app);

    let _ = app.update(Message::RulesEditor(R::SaveConflict));
    let s = session(&app);
    assert!(s.save_conflict);
    assert!(!s.save_target_gone);

    let _ = app.update(Message::RulesEditor(R::SaveTargetGone));
    let s = session(&app);
    assert!(s.save_target_gone);
}

/// SaveCompleted on a CREATE session morphs it into an EDIT session of the
/// new playlist (further Saves become updates; the observe loop gains a
/// target) and re-baselines the metadata dirty checks.
#[test]
fn save_completed_morphs_create_into_edit() {
    let mut app = capable_app();
    open_create(&mut app);
    if let Some(editor) = app.playlist_editor.as_mut() {
        editor.edit.set_name("Fresh Rules".into());
    }

    let _ = app.update(Message::RulesEditor(R::SaveCompleted {
        playlist_id: "new-1".into(),
        name: "Fresh Rules".into(),
        saved_updated_at: "T1".into(),
        detached: false,
        spun_off: false,
    }));

    let s = session(&app);
    assert!(
        matches!(&s.target, RulesTarget::Edit { playlist_id, .. } if playlist_id == "new-1"),
        "the create session now edits the new playlist"
    );
    assert!(!s.dirty);
    let editor = app.playlist_editor.as_ref().expect("mounted");
    assert_eq!(editor.edit.playlist_id, "new-1");
    assert!(!editor.edit.has_metadata_changes(), "metadata re-baselined");
}

// --- Action-level remaps ----------------------------------------------------

/// The resolved SaveQueueAsPlaylist action inside a rules session routes to
/// the rules Save — the queue dialog never opens (rebind-safe: keyed on the
/// ACTION, not the chord).
#[test]
fn resolved_save_action_remaps_to_rules_save() {
    let mut app = capable_app();
    open_edit(&mut app);
    app.toast.toasts.clear();

    let _ = app.handle_raw_key_event(
        iced::keyboard::Key::Character("s".into()),
        iced::keyboard::Modifiers::CTRL,
        iced::event::Status::Ignored,
    );

    assert!(
        !app.text_input_dialog.visible,
        "the queue save dialog must not open inside a rules session"
    );
    let s = session(&app);
    assert!(
        s.saving || app.toast.toasts.back().is_some(),
        "the rules Save path ran instead (in flight or validation-refused)"
    );
}

/// Shift+Tab hops panes (the deliberate Trawl divergence); Up/Down then
/// move the results cursor on the results side.
#[test]
fn shift_tab_hops_panes() {
    let mut app = capable_app();
    open_edit(&mut app);
    assert_eq!(session(&app).pane, RulesPane::Form);

    let _ = app.handle_raw_key_event(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
        iced::keyboard::Modifiers::SHIFT,
        iced::event::Status::Ignored,
    );
    assert_eq!(session(&app).pane, RulesPane::Results, "Shift+Tab hops");

    let _ = app.handle_raw_key_event(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
        iced::keyboard::Modifiers::SHIFT,
        iced::event::Status::Ignored,
    );
    assert_eq!(session(&app).pane, RulesPane::Form, "…both directions");
}

// --- EditCenteredPlaylist hotkey -------------------------------------------

/// The keyboard front door: on the centered Playlists row, kind-dispatched
/// — owned smart opens the rules session directly.
#[test]
fn edit_centered_playlist_opens_rules_for_owned_smart() {
    let mut app = capable_app();
    app.current_view = crate::View::Playlists;
    app.library
        .playlists
        .append_page(vec![smart_row("sp1", "Loved", "user-9")], 1);

    let _ = app.update(Message::Hotkey(
        crate::app_message::HotkeyMessage::EditCenteredPlaylist,
    ));

    assert!(
        app.rules_session().is_some(),
        "e on an owned smart row lands in the rules session"
    );
}

/// …and an unowned smart row refuses with the ownership toast.
#[test]
fn edit_centered_playlist_refuses_unowned_smart() {
    let mut app = capable_app();
    app.current_view = crate::View::Playlists;
    app.library
        .playlists
        .append_page(vec![smart_row("sp2", "Theirs", "user-OTHER")], 1);

    let _ = app.update(Message::Hotkey(
        crate::app_message::HotkeyMessage::EditCenteredPlaylist,
    ));

    assert!(app.rules_session().is_none());
    let toast = app.toast.toasts.back().expect("ownership toast");
    assert!(toast.message.contains("owner"));
}

// --- List-loaded gating -----------------------------------------------------

/// SessionPlaylistsLoaded flips the suppression gate: the duplicate-name
/// diagnostic fires only after the session list genuinely loads.
#[test]
fn session_list_gates_duplicate_name_diagnostic() {
    let mut app = capable_app();
    open_edit(&mut app);
    if let Some(editor) = app.playlist_editor.as_mut() {
        editor.edit.set_name("Road Trip".into());
    }
    app.revalidate_rules_session();
    assert!(
        !session(&app)
            .diagnostics
            .iter()
            .any(|d| d.message.contains("already exists")),
        "suppressed while the list is unloaded"
    );

    let _ = app.update(Message::RulesEditor(R::SessionPlaylistsLoaded(vec![
        ("p1".into(), "Road Trip".into()),
        ("sp1".into(), "Loved".into()),
    ])));
    assert!(
        session(&app)
            .diagnostics
            .iter()
            .any(|d| d.message.contains("already exists")),
        "…and fires once loaded"
    );
}

// ============================================================================
// M6 — .nsp import (collision routing + in-session load)
// ============================================================================

fn nsp_payload(name: &str) -> crate::update::nsp_import::NspPickResult {
    crate::update::nsp_import::NspPickResult::Parsed(Box::new(
        crate::update::nsp_import::NspImportPayload {
            name: name.into(),
            comment: "from file".into(),
            public: false,
            rules: serde_json::json!({
                "all": [ { "is": { "genre": "Soundtrack" } } ],
                "limit": 500
            }),
        },
    ))
}

/// Unclaimed name: no dialog opens (the create runs directly).
#[test]
fn nsp_import_no_collision_skips_dialog() {
    let mut app = capable_app();
    app.library
        .playlists
        .append_page(vec![smart_row("sp1", "Loved", "user-9")], 1);
    let _ = app.update(Message::NspImportPicked(nsp_payload("Fresh Name")));
    assert!(!app.text_input_dialog.visible, "direct create — no dialog");
}

/// Owned SMART collision: the three-way dialog (Update primary, Create new
/// extra, Cancel) with the imported name prefilled and public seeded from
/// the file.
#[test]
fn nsp_import_owned_smart_collision_opens_three_way() {
    let mut app = capable_app();
    app.library
        .playlists
        .append_page(vec![smart_row("sp1", "Loved", "user-9")], 1);
    let _ = app.update(Message::NspImportPicked(nsp_payload("Loved")));
    assert!(app.text_input_dialog.visible);
    assert_eq!(app.text_input_dialog.value, "Loved");
    assert!(!app.text_input_dialog.public, "seeded from the file");
    match &app.text_input_dialog.action {
        Some(crate::widgets::text_input_dialog::TextInputDialogAction::ImportNspUpdate {
            playlist_id,
            detach_sync,
            ..
        }) => {
            assert_eq!(playlist_id, "sp1");
            assert!(!detach_sync, "not file-backed");
        }
        other => panic!("expected ImportNspUpdate primary, got {other:?}"),
    }
    let (label, extra) = app
        .text_input_dialog
        .extra_action
        .as_ref()
        .expect("Create new alternative");
    assert_eq!(label, "Create new");
    assert!(matches!(
        extra,
        crate::widgets::text_input_dialog::TextInputDialogAction::ImportNspCreate { .. }
    ));
    assert!(
        app.text_input_dialog
            .note
            .as_deref()
            .is_some_and(|n| n.contains("already own a smart playlist")),
        "collision honesty note"
    );
}

/// Owned smart collision on a file-backed synced row with 0.62+ caps:
/// Update carries the detach (`sync: false`) and the note says so.
#[test]
fn nsp_import_detach_sync_gated_on_caps() {
    let mut app = capable_app();
    let mut row = smart_row("sp1", "Loved", "user-9");
    row.is_file_backed = true;
    row.sync = true;
    app.library.playlists.append_page(vec![row.clone()], 1);
    let _ = app.update(Message::NspImportPicked(nsp_payload("Loved")));
    assert!(matches!(
        app.text_input_dialog.action,
        Some(
            crate::widgets::text_input_dialog::TextInputDialogAction::ImportNspUpdate {
                detach_sync: true,
                ..
            }
        )
    ));
    assert!(
        app.text_input_dialog
            .note
            .as_deref()
            .is_some_and(|n| n.contains("detaches")),
        "detach honesty note"
    );

    // Same row on 0.61 caps: no sync PUT — detach stays off, and the note
    // states the scan re-sync instead.
    let mut app61 = capable_app();
    app61.caps_state = CapsState::Fetched(ServerCaps::from_version_str("0.61.0"));
    app61.library.playlists.append_page(vec![row], 1);
    let _ = app61.update(Message::NspImportPicked(nsp_payload("Loved")));
    assert!(matches!(
        app61.text_input_dialog.action,
        Some(
            crate::widgets::text_input_dialog::TextInputDialogAction::ImportNspUpdate {
                detach_sync: false,
                ..
            }
        )
    ));
    assert!(
        app61
            .text_input_dialog
            .note
            .as_deref()
            .is_some_and(|n| n.contains("every scan")),
        "0.61 re-sync honesty note"
    );
}

/// Unowned collision: create-only dialog (no Update offer) naming the owner.
#[test]
fn nsp_import_unowned_collision_creates_only() {
    let mut app = capable_app();
    app.library
        .playlists
        .append_page(vec![smart_row("sp9", "Loved", "someone-else")], 1);
    let _ = app.update(Message::NspImportPicked(nsp_payload("Loved")));
    assert!(app.text_input_dialog.visible);
    assert!(matches!(
        app.text_input_dialog.action,
        Some(crate::widgets::text_input_dialog::TextInputDialogAction::ImportNspCreate { .. })
    ));
    assert!(
        app.text_input_dialog.extra_action.is_none(),
        "no Update offer"
    );
    assert!(
        app.text_input_dialog
            .note
            .as_deref()
            .is_some_and(|n| n.contains("belongs to")),
        "names the other owner"
    );
}

/// Owned ORDINARY collision: create-only (a rules PUT would silently
/// convert the regular playlist to smart — never offered).
#[test]
fn nsp_import_owned_regular_collision_creates_only() {
    let mut app = capable_app();
    let mut row = smart_row("p1", "Loved", "user-9");
    row.is_smart = false;
    row.rules = None;
    app.library.playlists.append_page(vec![row], 1);
    let _ = app.update(Message::NspImportPicked(nsp_payload("Loved")));
    assert!(matches!(
        app.text_input_dialog.action,
        Some(crate::widgets::text_input_dialog::TextInputDialogAction::ImportNspCreate { .. })
    ));
    assert!(app.text_input_dialog.extra_action.is_none());
    assert!(
        app.text_input_dialog
            .note
            .as_deref()
            .is_some_and(|n| n.contains("ordinary")),
        "regular-name honesty note"
    );
}

/// A failed pick/parse toasts the reason and opens nothing.
#[test]
fn nsp_import_failure_toasts() {
    let mut app = capable_app();
    let _ = app.update(Message::NspImportPicked(
        crate::update::nsp_import::NspPickResult::Failed("couldn't parse JSON".into()),
    ));
    assert!(!app.text_input_dialog.visible);
    let toast = app.toast.toasts.back().expect("failure toast");
    assert_eq!(toast.level, ToastLevel::Error);
    assert!(toast.message.contains("couldn't parse JSON"));
}

/// In-session import (the create empty state's row): the parsed file seeds
/// the OPEN session — edit-bar metadata + rules + dirty, cursor on Match.
#[test]
fn nsp_parsed_seeds_open_create_session() {
    let mut app = capable_app();
    open_create(&mut app);
    let _ = app.update(Message::RulesEditor(R::NspParsed(nsp_payload(
        "Movie & TV Soundtracks",
    ))));
    let s = session(&app);
    assert!(s.dirty);
    assert_eq!(s.mode, FormMode::Cursor);
    assert!(
        s.rules.root.as_ref().is_some_and(|r| !r.nodes.is_empty()),
        "criteria landed in the tree"
    );
    assert!(matches!(s.rows.get(s.cursor), Some(FormRow::Match)));
    let editor = app.playlist_editor.as_ref().expect("editor mounted");
    assert_eq!(editor.edit.playlist_name, "Movie & TV Soundtracks");
    assert_eq!(editor.edit.playlist_comment, "from file");
    assert!(!editor.edit.playlist_public, "public seeded from the file");
}

// ============================================================================
// Review-round regression guards (adversarial review, 2026-07-18)
// ============================================================================

/// The edit-bar Public cell is reachable by keyboard: Enter on it flips
/// visibility through the shared `set_public` write path.
#[test]
fn enter_on_public_cell_toggles_visibility() {
    let mut app = capable_app();
    open_create(&mut app);
    // Land the cursor on the edit-bar Public cell in Cursor mode.
    app.with_rules_session(|s| {
        s.mode = FormMode::Cursor;
        s.editing = None;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::EditBar))
            .unwrap_or(0);
        s.cell = FormCell::Public;
    });
    let before = app
        .playlist_editor
        .as_ref()
        .expect("editor")
        .edit
        .playlist_public;
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let after = app
        .playlist_editor
        .as_ref()
        .expect("editor")
        .edit
        .playlist_public;
    assert_ne!(before, after, "Enter on Public flips visibility");
}

/// "Save as new" FROM an edit session (spun_off) leaves the original
/// session's target, token, and draft intact — no false conflict, no leak.
#[test]
fn save_completed_spun_off_preserves_original_session() {
    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.draft = Some(nokkvi_data::types::rules_session::DraftInfo {
            id: "draft-1".into(),
            marker: "m".into(),
        });
        s.dirty = true;
    });
    let target_before = format!("{:?}", session(&app).target);

    let _ = app.update(Message::RulesEditor(R::SaveCompleted {
        playlist_id: "copy-99".into(),
        name: "Loved (copy)".into(),
        saved_updated_at: "COPY_STAMP".into(),
        detached: false,
        spun_off: true,
    }));

    let s = session(&app);
    assert!(!s.saving, "the in-flight flag clears");
    assert!(s.dirty, "the original still has unsaved edits");
    assert!(
        s.draft.as_ref().is_some_and(|d| d.id == "draft-1"),
        "the original draft handle is retained (cleaned on close), not leaked"
    );
    assert_eq!(
        format!("{:?}", s.target),
        target_before,
        "the session keeps editing the ORIGINAL — the copy's id is ignored"
    );
}

/// Entering rules mode while an editor is already open is refused (no
/// clobber of the live session / draft orphan).
#[test]
fn enter_rules_mode_refuses_over_open_editor() {
    let mut app = capable_app();
    open_create(&mut app);
    let toasts_before = app.toast.toasts.len();
    // A second create entry (e.g. Shift+N while already in a session).
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterRulesMode {
        target: RulesEntryTarget::Create,
    }));
    assert!(
        app.toast.toasts.len() > toasts_before,
        "the second entry is refused with a toast"
    );
    assert!(
        matches!(session(&app).target, RulesTarget::Create),
        "the original session survives"
    );
}

/// A root modal open OVER the rules split-view (here the Trawl builder) owns
/// the keyboard — Escape must NOT reach the hidden rules form and arm its
/// discard confirm (the intercept-before-modal-gate regression).
#[test]
fn modal_over_rules_session_owns_the_keyboard() {
    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| s.dirty = true);
    // Trawl builder opens on top (the `t` hotkey is view-agnostic).
    app.trawl_modal = Some(crate::widgets::trawl_modal::TrawlModalState::default());

    let _ = app.handle_raw_key_event(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        iced::keyboard::Modifiers::default(),
        iced::event::Status::Ignored,
    );

    assert!(
        !session(&app).confirm_discard,
        "Escape belongs to the modal — the hidden form's discard confirm must stay disarmed"
    );
}

/// Exiting the editor bumps `rules_preview_generation` so an in-flight
/// preview task can't be adopted by the NEXT session (finding 6).
#[test]
fn exiting_rules_mode_invalidates_preview_generation() {
    let mut app = capable_app();
    open_edit(&mut app);
    let before = app.rules_preview_generation;
    let _ = app.handle_exit_playlist_edit_mode();
    assert!(app.playlist_editor.is_none(), "session torn down");
    assert!(
        app.rules_preview_generation > before,
        "close invalidates in-flight preview tasks (they can't seed the next session)"
    );
}

/// Discarding while a save is in flight refuses rather than deleting the
/// draft the save is promoting into the real playlist (finding 5).
#[test]
fn confirm_discard_while_saving_preserves_the_draft() {
    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.draft = Some(nokkvi_data::types::rules_session::DraftInfo {
            id: "draft-1".into(),
            marker: "m".into(),
        });
        s.saving = true;
        s.confirm_discard = true;
    });
    let _ = app.update(Message::RulesEditor(R::ConfirmDiscard));
    assert!(app.playlist_editor.is_some(), "discard is refused mid-save");
    assert!(
        session(&app)
            .draft
            .as_ref()
            .is_some_and(|d| d.id == "draft-1"),
        "the draft the promote-PUT is finalizing is left intact"
    );
}

/// A create session can't upload a cover (no server-side playlist yet):
/// SetCover toasts "save first" instead of opening the picker.
#[test]
fn set_cover_on_create_toasts_save_first() {
    let mut app = capable_app();
    open_create(&mut app);
    let _ = app.update(Message::RulesEditor(R::SetCover));
    let toast = app.toast.toasts.back().expect("a toast");
    assert!(
        toast.message.contains("Save the playlist first"),
        "got: {}",
        toast.message
    );
}

/// A genre (tag) value cell opens a value PICKER of the library's genres
/// rather than a text input — server tag matching is case-sensitive, so
/// typing "phonk" would miss "Phonk".
#[test]
fn genre_value_opens_a_tag_picker() {
    use nokkvi_data::types::smart_criteria::{SmartRules, TagDiscovery};

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "is": { "genre": "" } } ]
        }));
        s.tag_discovery = Some(TagDiscovery::from_rows(vec![
            ("genre".to_string(), "Phonk".to_string(), 340u64),
            ("genre".to_string(), "Soundtrack".to_string(), 120u64),
        ]));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });

    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let s = session(&app);
    match s.sub_picker.as_ref().map(|p| &p.kind) {
        Some(crate::state::SubPickerKind::TagValue { tag, .. }) => assert_eq!(tag, "genre"),
        other => panic!("expected a TagValue picker, got {other:?}"),
    }
    let picker = s.sub_picker.as_ref().expect("picker");
    let entries = crate::update::rules_editor::rules_picker_entries(s, picker);
    assert!(entries.iter().any(|(v, _)| v == "Phonk"), "lists Phonk");
    assert!(entries.iter().any(|(v, _)| v == "Soundtrack"));
}

/// A rating value cell opens a fixed 0–5 star picker (Navidrome ratings are
/// discrete), not a typed number.
#[test]
fn rating_value_opens_a_star_picker() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "gt": { "rating": 2 } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });

    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let s = session(&app);
    assert!(
        matches!(
            s.sub_picker.as_ref().map(|p| &p.kind),
            Some(crate::state::SubPickerKind::RatingValue { .. })
        ),
        "a rating value opens the star picker"
    );
    let picker = s.sub_picker.as_ref().expect("picker");
    let entries = crate::update::rules_editor::rules_picker_entries(s, picker);
    let values: Vec<&str> = entries.iter().map(|(v, _)| v.as_str()).collect();
    assert_eq!(values, ["0", "1", "2", "3", "4", "5"], "0–5 stars");
}

/// Clicking a different cell while a value is being edited COMMITS the edit
/// first, then runs the clicked cell's action — so one click both saves the
/// typed value and opens the operator picker (the "click does nothing while
/// a field is active" bug).
#[test]
fn clicking_a_cell_while_editing_commits_then_acts() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        // playcount is a plain Number field (text-edited, not a picker).
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "gt": { "playcount": 5 } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    assert_eq!(
        session(&app).mode,
        FormMode::Editing,
        "value cell edits as text"
    );
    let _ = app.update(Message::RulesEditor(R::EditingInput("10".into())));

    let rule_row = session(&app)
        .rows
        .iter()
        .position(|r| matches!(r, FormRow::Rule(_)))
        .unwrap();
    let _ = app.update(Message::RulesEditor(R::ClickCell {
        row: rule_row,
        cell: FormCell::Operator,
    }));

    let s = session(&app);
    assert_eq!(
        s.rules.to_value()["all"][0]["gt"]["playcount"],
        10,
        "the pending edit committed on the click-away"
    );
    assert!(
        matches!(
            s.sub_picker.as_ref().map(|p| &p.kind),
            Some(crate::state::SubPickerKind::Operator { .. })
        ),
        "the same click opened the operator picker (not a wasted click)"
    );
}

/// Mouse-clicking Save while a value cell is mid-edit must flush the typed
/// value FIRST — the click blurs the input without committing, so without the
/// flush Save would persist the pre-edit value and clear dirty (silent loss).
#[test]
fn save_flushes_a_pending_value_edit() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "gt": { "playcount": 5 } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let _ = app.update(Message::RulesEditor(R::EditingInput("10".into())));
    // The typed value lives only in the editing buffer — Save must commit it.
    let _ = app.update(Message::RulesEditor(R::Save));
    assert_eq!(
        session(&app).rules.to_value()["all"][0]["gt"]["playcount"],
        10,
        "Save flushed the pending edit before reading the rules"
    );
}

/// Same omission on Preview: clicking Preview mid-edit must evaluate the
/// typed rules, not the stale pre-edit ones.
#[test]
fn preview_flushes_a_pending_value_edit() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "gt": { "playcount": 5 } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let _ = app.update(Message::RulesEditor(R::EditingInput("10".into())));
    let _ = app.update(Message::RulesEditor(R::Preview));
    assert_eq!(
        session(&app).rules.to_value()["all"][0]["gt"]["playcount"],
        10,
        "Preview flushed the pending edit before reading the rules"
    );
}

/// The Discard chip dispatches EscapePressed. Clicked mid-edit it must leave
/// Editing mode BEFORE raising the confirm — otherwise the key intercept's
/// Editing block shadows the confirm branch and Enter/Escape mis-route.
#[test]
fn discard_chip_mid_edit_exits_editing_then_confirms() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "gt": { "playcount": 5 } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
        s.dirty = true;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    assert_eq!(session(&app).mode, FormMode::Editing);
    let _ = app.update(Message::RulesEditor(R::EscapePressed));
    let s = session(&app);
    assert_eq!(s.mode, FormMode::Cursor, "dropped out of Editing");
    assert!(s.confirm_discard, "and raised the discard confirm");
}

/// A range (Pair) value cell on a tag field must NOT open the scalar tag
/// picker — committing one entry would clobber the `[x, y]` array. Such a
/// combo is reachable by cycling the field after picking `inTheRange`.
#[test]
fn range_tag_value_cell_does_not_open_the_scalar_tag_picker() {
    use nokkvi_data::types::smart_criteria::{SmartRules, TagDiscovery};

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "inTheRange": { "genre": [1, 5] } } ]
        }));
        s.tag_discovery = Some(TagDiscovery::from_rows(vec![(
            "genre".to_string(),
            "Phonk".to_string(),
            340u64,
        )]));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let s = session(&app);
    assert!(
        s.sub_picker.is_none(),
        "a Pair-shaped tag value edits as text, never the scalar tag picker"
    );
    assert_eq!(
        s.rules.to_value()["all"][0]["inTheRange"]["genre"],
        serde_json::json!([1, 5]),
        "the range array is left intact (not clobbered to a scalar)"
    );
}

/// Cycling a rule's field onto a multi-valued tag while `inTheRange` is set
/// must snap the operator to a valid one — a tag never keeps the range
/// operator (which would render two numeric inputs over a tag).
#[test]
fn changing_field_to_a_tag_snaps_off_the_range_operator() {
    use nokkvi_data::types::smart_criteria::{RuleOperator, SmartRules};

    let mut app = capable_app();
    open_edit(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "inTheRange": { "rating": [1, 5] } } ]
        }));
        s.rebuild_rows();
        s.set_leaf_field(&[0], "genre");
    });
    let s = session(&app);
    let Some(CriteriaNode::Leaf(leaf)) = s.node_at(&[0]) else {
        panic!("leaf expected");
    };
    assert_eq!(leaf.field, "genre");
    assert_ne!(
        leaf.operator,
        RuleOperator::InTheRange,
        "the range operator snapped off the multi-valued tag"
    );
    assert!(
        s.valid_operators_for_row(&[0]).contains(&leaf.operator),
        "the snapped-to operator is valid for the new field"
    );
}

/// Blank create shows the Start-empty / Import / preset list — Up/Down walk
/// its OWN cursor (not the hidden form rows), and Enter activates the entry
/// under it. Here index 0 (Start empty) seeds an empty editable session.
#[test]
fn blank_create_empty_state_is_keyboard_navigable() {
    let mut app = capable_app();
    open_create(&mut app);
    assert!(session(&app).is_blank_create());
    assert_eq!(session(&app).empty_state_cursor, 0);

    // Up at the top clamps; Down advances.
    let _ = app.update(Message::RulesEditor(R::EmptyStateMove { down: false }));
    assert_eq!(session(&app).empty_state_cursor, 0);
    let _ = app.update(Message::RulesEditor(R::EmptyStateMove { down: true }));
    assert_eq!(session(&app).empty_state_cursor, 1);

    // Back to the top and activate → Start empty (dirties, leaves blank state).
    let _ = app.update(Message::RulesEditor(R::EmptyStateMove { down: false }));
    let _ = app.update(Message::RulesEditor(R::EmptyStateActivate));
    let s = session(&app);
    assert!(s.dirty, "activating Start empty seeded a real session");
    assert!(!s.is_blank_create(), "no longer the blank empty-state");
}

/// Escape while editing the edit-bar Name reverts the CELL — restoring the
/// pre-edit name through the same write path the keystrokes used (the inputs
/// live-commit, so there is no buffer to simply drop).
#[test]
fn escape_reverts_the_edit_bar_name() {
    let mut app = capable_app();
    open_edit(&mut app);
    let original = app
        .playlist_editor
        .as_ref()
        .expect("editor")
        .edit
        .playlist_name
        .clone();

    app.with_rules_session(|s| {
        s.mode = FormMode::Cursor;
        s.cursor = 0;
        s.cell = FormCell::Name;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    assert_eq!(session(&app).mode, FormMode::Editing);

    // Live-type a replacement (as the focused input would).
    let _ = app.update(Message::Editor(
        crate::app_message::EditorMessage::NameChanged("Changed".into()),
    ));
    assert_eq!(
        app.playlist_editor.as_ref().unwrap().edit.playlist_name,
        "Changed"
    );

    // Escape restores the snapshot captured on entry.
    let _ = app.update(Message::RulesEditor(R::RevertEditing));
    assert_eq!(
        app.playlist_editor.as_ref().unwrap().edit.playlist_name,
        original,
        "Escape put the pre-edit name back"
    );
}

// --- date picker -----------------------------------------------------------

/// A Date-class value cell opens a calendar picker seeded from the cell's
/// current date (displayed month + focused day), not a text input.
#[test]
fn date_value_opens_a_calendar_picker() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "is": { "dateadded": "2026-07-15" } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let s = session(&app);
    match s.sub_picker.as_ref().map(|p| &p.kind) {
        Some(crate::state::SubPickerKind::DateValue {
            year, month, slot2, ..
        }) => {
            assert_eq!(
                (*year, *month, *slot2),
                (2026, 7, false),
                "seeded from value"
            );
        }
        other => panic!("expected a DateValue picker, got {other:?}"),
    }
    assert_eq!(
        s.sub_picker.as_ref().unwrap().cursor,
        15,
        "focused day = the current day"
    );
}

/// Picking a day (mouse) commits the exact YYYY-MM-DD for the picked day in
/// the displayed month and closes the calendar. Month nav is honored.
#[test]
fn date_picker_pick_commits_the_ymd_string() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "before": { "lastplayed": "2026-07-15" } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let _ = app.update(Message::RulesEditor(R::DatePickerShiftMonth {
        forward: false,
    }));
    let _ = app.update(Message::RulesEditor(R::DatePickerPickDay(3)));
    let s = session(&app);
    assert!(s.sub_picker.is_none(), "picking closes the calendar");
    assert_eq!(
        s.rules.to_value()["all"][0]["before"]["lastplayed"],
        "2026-06-03",
        "committed the picked day in the shifted-back month"
    );
}

/// Editing the second endpoint (Value2) of a date range commits only that
/// slot, leaving the first bound intact.
#[test]
fn date_range_picks_into_the_second_slot() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "inTheRange": { "dateadded": ["2020-01-01", "2020-12-31"] } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value2;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    match session(&app).sub_picker.as_ref().map(|p| &p.kind) {
        Some(crate::state::SubPickerKind::DateValue {
            slot2, year, month, ..
        }) => {
            assert!(*slot2, "the Value2 cell edits the second endpoint");
            assert_eq!((*year, *month), (2020, 12), "seeded from the 'to' bound");
        }
        other => panic!("expected DateValue, got {other:?}"),
    }
    let _ = app.update(Message::RulesEditor(R::DatePickerPickDay(15)));
    assert_eq!(
        session(&app).rules.to_value()["all"][0]["inTheRange"]["dateadded"],
        serde_json::json!(["2020-01-01", "2020-12-15"]),
        "only the second slot changed"
    );
}

/// Keyboard: MoveDay rolls the focused day (across month boundaries) and
/// Commit writes the focused day.
#[test]
fn date_picker_keyboard_moves_and_commits() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "after": { "lastplayed": "2026-07-15" } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    // Focused day 15 → +7 → 22, then Enter.
    let _ = app.update(Message::RulesEditor(R::DatePickerMoveDay { by: 7 }));
    let _ = app.update(Message::RulesEditor(R::DatePickerCommit));
    let s = session(&app);
    assert!(s.sub_picker.is_none());
    assert_eq!(
        s.rules.to_value()["all"][0]["after"]["lastplayed"],
        "2026-07-22"
    );
}

/// Stepping to a shorter month keeps the focused day valid (Jan 31 → Feb 28).
#[test]
fn date_picker_month_shift_clamps_the_focused_day() {
    use nokkvi_data::types::smart_criteria::SmartRules;

    let mut app = capable_app();
    open_create(&mut app);
    app.with_rules_session(|s| {
        s.rules = SmartRules::parse(&serde_json::json!({
            "all": [ { "before": { "lastplayed": "2026-01-31" } } ]
        }));
        s.rebuild_rows();
        s.mode = FormMode::Cursor;
        s.cursor = s
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(_)))
            .expect("rule row");
        s.cell = FormCell::Value;
    });
    let _ = app.update(Message::RulesEditor(R::EnterOnCursor));
    let _ = app.update(Message::RulesEditor(R::DatePickerShiftMonth {
        forward: true,
    }));
    let s = session(&app);
    match s.sub_picker.as_ref().map(|p| &p.kind) {
        Some(crate::state::SubPickerKind::DateValue { year, month, .. }) => {
            assert_eq!((*year, *month), (2026, 2));
        }
        other => panic!("expected DateValue, got {other:?}"),
    }
    assert_eq!(
        s.sub_picker.as_ref().unwrap().cursor,
        28,
        "Jan 31 clamped to Feb 28"
    );
}
