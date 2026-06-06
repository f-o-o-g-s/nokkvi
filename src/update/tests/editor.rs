//! Tests for the playlist-editor handlers.
//!
//! The editor owns its OWN in-memory track buffer (`Nokkvi.playlist_editor`),
//! fully decoupled from the live play queue. These tests assert that entering
//! edit mode no longer touches the queue, that `SongsLoaded` populates the
//! buffer and seeds a clean dirty snapshot, and that creating a new (empty)
//! playlist for edit leaves a populated queue intact.

use nokkvi_data::types::playlist_edit::PlaylistEditState;

use crate::{
    app_message::{EditorMessage, Message, SplitViewMessage},
    state::PlaylistEditorState,
    test_helpers::*,
    views::queue::QueueContextEntry,
    widgets::drag_column::DragEvent,
};

/// Enter edit mode via the same message the UI dispatches.
fn enter_edit(app: &mut crate::Nokkvi, playlist_id: &str) {
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: playlist_id.to_string(),
        playlist_name: "Test Playlist".to_string(),
        playlist_comment: String::new(),
        playlist_public: false,
    }));
}

#[test]
fn entering_edit_mode_does_not_touch_queue() {
    // Seed a non-empty live queue, then enter edit mode. The synchronous enter
    // path must NOT mutate the queue — the async resolve task that fills the
    // editor buffer doesn't run in test_app, which is fine; we only assert the
    // queue is left byte-for-byte unchanged.
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("q1", "Queue One", "Artist A", "Album A"),
        make_queue_song("q2", "Queue Two", "Artist B", "Album B"),
    ];
    let before = app.queue_song_ids();

    enter_edit(&mut app, "pl_1");

    assert_eq!(
        app.queue_song_ids(),
        before,
        "entering edit mode must not mutate the live queue"
    );
    assert!(
        app.playlist_editor.is_some(),
        "entering edit mode must create an editor session"
    );
}

#[test]
fn entering_edit_mode_collapses_playlist_strip() {
    // The "Playing From" banner's hover `mouse_area` is unmounted the moment
    // edit mode swaps the queue pane out for the editor, so its `on_exit` can
    // never fire to collapse a hover-expanded strip. Edit-mode entry must reset
    // the expansion flag explicitly (a reset hook alongside
    // `clear_active_playlist`), or the banner re-mounts stale-expanded with no
    // cursor over it (e.g. on the Queue tab mid-edit).
    let mut app = test_app();
    app.queue_page.playlist_strip_expanded = true;

    enter_edit(&mut app, "pl_1");

    assert!(
        !app.queue_page.playlist_strip_expanded,
        "entering edit mode must collapse the playlist strip (its hover on_exit \
         can never fire once the banner is unmounted)"
    );
}

#[test]
fn exiting_edit_mode_collapses_playlist_strip() {
    // Symmetric to the enter edge: leaving edit mode re-mounts the queue banner.
    // A strip expansion stranded true during the session must not survive the
    // return, or the banner appears fully expanded with no cursor over it.
    let mut app = test_app();
    enter_edit(&mut app, "pl_1");
    app.queue_page.playlist_strip_expanded = true;

    let _ = app.update(Message::SplitView(SplitViewMessage::ExitEditMode));

    assert!(
        !app.queue_page.playlist_strip_expanded,
        "exiting edit mode must re-mount the queue banner collapsed"
    );
}

#[test]
fn editor_songs_loaded_populates_buffer() {
    let mut app = test_app();
    app.playlist_editor = Some(PlaylistEditorState::new(PlaylistEditState::new(
        "pl_1".into(),
        "Test Playlist".into(),
        String::new(),
        false,
        Vec::new(),
    )));

    let row_a = make_queue_song("a", "Song A", "Artist", "Album");
    let row_b = make_queue_song("b", "Song B", "Artist", "Album");
    let row_c = make_queue_song("c", "Song C", "Artist", "Album");

    let _ = app.update(Message::Editor(EditorMessage::SongsLoaded(vec![
        row_a, row_b, row_c,
    ])));

    let ids: Vec<&str> = app
        .playlist_editor
        .as_ref()
        .expect("editor session present")
        .songs
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["a", "b", "c"],
        "SongsLoaded must populate the editor buffer in order"
    );
}

#[test]
fn freshly_loaded_editor_is_not_dirty() {
    // Regression for bug 10: PlaylistEditState::new seeds an EMPTY snapshot, so
    // a freshly-loaded session is "dirty" against the loaded rows until the
    // snapshot is re-seeded on SongsLoaded.
    let mut app = test_app();
    app.playlist_editor = Some(PlaylistEditorState::new(PlaylistEditState::new(
        "pl_1".into(),
        "Test Playlist".into(),
        String::new(),
        false,
        Vec::new(),
    )));

    let _ = app.update(Message::Editor(EditorMessage::SongsLoaded(vec![
        make_queue_song("a", "Song A", "Artist", "Album"),
        make_queue_song("b", "Song B", "Artist", "Album"),
    ])));

    let editor_ids = app.editor_song_ids();
    assert_eq!(
        editor_ids.len(),
        2,
        "SongsLoaded must fill the buffer before dirty-detection is meaningful"
    );
    let is_dirty = app
        .playlist_editor
        .as_ref()
        .expect("editor session present")
        .edit
        .is_dirty(&editor_ids);
    assert!(
        !is_dirty,
        "a freshly-loaded editor must be clean (snapshot seeded from loaded rows)"
    );
}

#[test]
fn create_new_playlist_edit_does_not_clear_queue() {
    // Regression for bug 4: "create new playlist & edit" with a populated queue
    // previously called set_queue(Vec::new(), None), wiping the queue (and disk).
    // Entering edit for an empty playlist must leave the queue untouched.
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("q1", "Queue One", "Artist A", "Album A"),
        make_queue_song("q2", "Queue Two", "Artist B", "Album B"),
        make_queue_song("q3", "Queue Three", "Artist C", "Album C"),
    ];
    let before = app.queue_song_ids();

    enter_edit(&mut app, "pl_new_empty");

    assert_eq!(
        app.queue_song_ids(),
        before,
        "creating a new (empty) playlist for edit must not clear the queue"
    );
}

/// Seed a clean editor session with a small loaded buffer.
fn seeded_editor(app: &mut crate::Nokkvi) {
    app.playlist_editor = Some(PlaylistEditorState::new(PlaylistEditState::new(
        "pl_1".into(),
        "Test Playlist".into(),
        "Original comment".into(),
        true,
        Vec::new(),
    )));
    let _ = app.update(Message::Editor(EditorMessage::SongsLoaded(vec![
        make_queue_song("a", "Song A", "Artist", "Album"),
        make_queue_song("b", "Song B", "Artist", "Album"),
        make_queue_song("c", "Song C", "Artist", "Album"),
    ])));
}

#[test]
fn editor_name_changed_marks_name_dirty() {
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::NameChanged(
        "Renamed Playlist".into(),
    )));

    let edit = &app
        .playlist_editor
        .as_ref()
        .expect("editor session present")
        .edit;
    assert_eq!(edit.playlist_name, "Renamed Playlist");
    assert!(
        edit.is_name_dirty(),
        "changing the name must flip the name-dirty flag"
    );
}

#[test]
fn editor_comment_changed_marks_comment_dirty() {
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::CommentChanged(
        "Updated comment".into(),
    )));

    let edit = &app
        .playlist_editor
        .as_ref()
        .expect("editor session present")
        .edit;
    assert_eq!(edit.playlist_comment, "Updated comment");
    assert!(
        edit.is_comment_dirty(),
        "changing the comment must flip the comment-dirty flag"
    );
}

#[test]
fn comment_only_edit_leaves_name_and_public_clean() {
    // N21: a comment-only edit must NOT mark name or public dirty — proving the
    // save path will omit them from the wire (so it cannot re-write the name or
    // silently revert a concurrent visibility change). Observable PlaylistEditState.
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::CommentChanged(
        "Updated comment".into(),
    )));

    let edit = &app
        .playlist_editor
        .as_ref()
        .expect("editor session present")
        .edit;
    assert!(
        edit.is_comment_dirty(),
        "the comment edit must dirty the comment"
    );
    assert!(
        !edit.is_name_dirty(),
        "a comment-only edit must leave the name clean (save omits it)"
    );
    assert!(
        !edit.is_public_dirty(),
        "a comment-only edit must leave public clean (save omits it — no silent revert)"
    );
}

#[test]
fn editor_public_toggled_marks_public_dirty() {
    let mut app = test_app();
    seeded_editor(&mut app);
    // Seeded session starts public = true.

    let _ = app.update(Message::Editor(EditorMessage::PublicToggled(false)));

    let edit = &app
        .playlist_editor
        .as_ref()
        .expect("editor session present")
        .edit;
    assert!(!edit.playlist_public);
    assert!(
        edit.is_public_dirty(),
        "toggling public must flip the public-dirty flag"
    );
}

#[test]
fn editor_slot_selection_toggle_updates_selection() {
    use crate::widgets::SlotListPageMessage;

    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::SlotList(
        SlotListPageMessage::SelectionToggle(1),
    )));

    let editor = app
        .playlist_editor
        .as_ref()
        .expect("editor session present");
    assert!(
        editor.common.slot_list.selected_indices.contains(&1),
        "selection toggle must add the index to the editor's own slot-list selection"
    );
}

// --- N22: editor load-state lifecycle gate -------------------------------

#[test]
fn fresh_editor_session_starts_loading() {
    // Entering edit mode constructs the editor BEFORE the async resolve
    // returns. In test_app the resolve never runs, so the session must stay
    // Loading — distinguishing an in-flight resolve from a genuinely-empty
    // playlist.
    use crate::state::EditorLoadState;

    let mut app = test_app();
    enter_edit(&mut app, "pl_1");

    assert_eq!(
        app.playlist_editor
            .as_ref()
            .expect("editor session present")
            .load_state,
        EditorLoadState::Loading,
        "a freshly-entered editor must start in the Loading state"
    );
}

#[test]
fn editor_songs_loaded_marks_loaded() {
    use crate::state::EditorLoadState;

    let mut app = test_app();
    enter_edit(&mut app, "pl_1");

    let _ = app.update(Message::Editor(EditorMessage::SongsLoaded(vec![
        make_queue_song("a", "Song A", "Artist", "Album"),
    ])));

    assert_eq!(
        app.playlist_editor
            .as_ref()
            .expect("editor session present")
            .load_state,
        EditorLoadState::Loaded,
        "a successful resolve must mark the session Loaded"
    );
}

#[test]
fn editor_songs_load_failed_marks_failed() {
    use crate::state::EditorLoadState;

    let mut app = test_app();
    enter_edit(&mut app, "pl_1");

    let _ = app.update(Message::Editor(EditorMessage::SongsLoadFailed));

    assert_eq!(
        app.playlist_editor
            .as_ref()
            .expect("editor session present")
            .load_state,
        EditorLoadState::Failed,
        "a failed resolve must mark the session Failed (editor stays mounted)"
    );
    // The session is NOT auto-aborted — the editor remains for reload/discard.
    assert!(
        app.playlist_editor.is_some(),
        "a failed resolve must keep the editor mounted (no auto-abort)"
    );
}

#[test]
fn save_blocked_until_loaded() {
    // A save dispatched while the session is still Loading must early-return:
    // the empty buffer is NOT the real playlist, so persisting it would
    // full-overwrite the server. Observable: the editor session is preserved
    // and untouched (no replace task is built).
    let mut app = test_app();
    enter_edit(&mut app, "pl_1");
    app.active_playlist_info = None;

    // Session is Loading (the resolve never ran in test_app).
    let _ = app.update(Message::SplitView(SplitViewMessage::SavePlaylistEdits));

    assert!(
        app.playlist_editor.is_some(),
        "a save blocked on a not-yet-loaded session must preserve the editor"
    );
    // A warn toast tells the user to wait.
    assert_eq!(
        app.toast.toasts.len(),
        1,
        "a blocked save must surface exactly one warn toast"
    );
    assert_eq!(
        app.toast.toasts[0].level,
        nokkvi_data::types::toast::ToastLevel::Warning,
        "the blocked-save toast must be a warning"
    );
}

#[test]
fn failed_session_blocks_track_mutations() {
    // A Failed session must not accumulate edits: a remove on a failed-load
    // session is a no-op (the buffer is unreliable).
    use crate::state::EditorLoadState;

    let mut app = test_app();
    seeded_editor(&mut app); // Loaded with [a,b,c]
    // Force the session Failed after loading to model a mid-session failure.
    if let Some(editor) = app.playlist_editor.as_mut() {
        editor.load_state = EditorLoadState::Failed;
    }
    let before = editor_ids(&app);

    let _ = app.update(Message::Editor(EditorMessage::RemoveAt(1)));

    assert_eq!(
        editor_ids(&app),
        before,
        "a Failed session must block track mutations (buffer is unreliable)"
    );
}

#[test]
fn save_with_no_track_changes_preserves_clean_session() {
    // N10 Layer 1: a save on a clean (loaded, non-dirty) editor must skip the
    // destructive track overwrite. In test_app the replace task never runs
    // (no app_service), so the observable guarantee is that the editor session
    // is preserved and reports not-dirty — the gate decision did not corrupt
    // the session.
    let mut app = test_app();
    seeded_editor(&mut app); // Loaded, clean [a,b,c]
    assert!(
        !app.playlist_editor
            .as_ref()
            .unwrap()
            .edit
            .is_dirty(&app.editor_song_ids()),
        "freshly-loaded editor must be clean before the save"
    );

    let _ = app.update(Message::SplitView(SplitViewMessage::SavePlaylistEdits));

    assert!(
        app.playlist_editor.is_some(),
        "a clean save must preserve the editor session"
    );
    assert!(
        !app.playlist_editor
            .as_ref()
            .unwrap()
            .edit
            .is_dirty(&app.editor_song_ids()),
        "a no-track-change save must leave the session clean (no spurious dirty)"
    );
}

// --- Phase 4: buffer mutations (reorder / remove) ------------------------

/// Read the editor buffer's song IDs in order.
fn editor_ids(app: &crate::Nokkvi) -> Vec<String> {
    app.playlist_editor
        .as_ref()
        .expect("editor session present")
        .songs
        .iter()
        .map(|s| s.id.clone())
        .collect()
}

#[test]
fn editor_drag_reorder_moves_row() {
    // [a,b,c] — drag row 0 (a) so it lands after b (drop target index 2).
    // Insert-before semantics: dropping at target 2 with from < to lands the
    // dragged row at index 1, yielding [b,a,c]. Small buffers top-pack, so
    // slot index == item index (slot_count > 3).
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::DragReorder(
        DragEvent::Dropped {
            index: 0,
            target_index: 2,
        },
    )));

    assert_eq!(
        editor_ids(&app),
        vec!["b", "a", "c"],
        "drag-reorder must move the dragged row to its new buffer position"
    );
}

#[test]
fn editor_remove_at_deletes_row() {
    // RemoveAt(1) on [a,b,c] → [a,c].
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::RemoveAt(1)));

    assert_eq!(
        editor_ids(&app),
        vec!["a", "c"],
        "RemoveAt must delete the single targeted row"
    );
}

#[test]
fn editor_becomes_dirty_after_reorder() {
    let mut app = test_app();
    seeded_editor(&mut app);

    // Freshly loaded: clean.
    assert!(
        !app.playlist_editor
            .as_ref()
            .unwrap()
            .edit
            .is_dirty(&app.editor_song_ids()),
        "a freshly-loaded editor must be clean before any mutation"
    );

    let _ = app.update(Message::Editor(EditorMessage::DragReorder(
        DragEvent::Dropped {
            index: 0,
            target_index: 2,
        },
    )));

    assert!(
        app.playlist_editor
            .as_ref()
            .unwrap()
            .edit
            .is_dirty(&app.editor_song_ids()),
        "reordering the buffer must make the session dirty (computed at render time)"
    );
}

#[test]
fn editor_context_menu_remove_deletes_selection() {
    // Multi-select aware (mirrors the queue's RemoveFromQueue): select rows 0
    // and 2, then trigger the editor remove path on one of them. Both removed.
    use crate::widgets::SlotListPageMessage;

    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::SlotList(
        SlotListPageMessage::SelectionToggle(0),
    )));
    let _ = app.update(Message::Editor(EditorMessage::SlotList(
        SlotListPageMessage::SelectionToggle(2),
    )));

    // RemoveAt on a row that IS part of the selection removes the whole
    // selection (evaluate_context_menu semantics, like the queue).
    let _ = app.update(Message::Editor(EditorMessage::RemoveAt(2)));

    assert_eq!(
        editor_ids(&app),
        vec!["b"],
        "removing a selected row must remove the whole multi-selection"
    );
    // Selection is cleared after the batch remove (mirrors the queue).
    assert!(
        app.playlist_editor
            .as_ref()
            .unwrap()
            .common
            .slot_list
            .selected_indices
            .is_empty(),
        "selection must be cleared after a batch remove"
    );
}

#[test]
fn editor_context_menu_action_remove_deletes_row() {
    // The context-menu "Remove from Playlist" entry also routes through the
    // ContextMenuAction(idx, RemoveFromQueue) path; assert it removes the row.
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::ContextMenuAction(
        0,
        QueueContextEntry::RemoveFromQueue,
    )));

    assert_eq!(
        editor_ids(&app),
        vec!["b", "c"],
        "ContextMenuAction(RemoveFromQueue) must delete the targeted row"
    );
}

#[test]
fn editor_reorder_under_active_search_is_guarded() {
    // Invariant #1: with a search query active, slot indices are relative to a
    // filtered view, so a raw-index reorder could move the wrong row. Mirror
    // the queue's guard (views/queue/update.rs:119) — reorder is a no-op while
    // a search query is active; the buffer is left uncorrupted.
    let mut app = test_app();
    seeded_editor(&mut app);

    if let Some(editor) = app.playlist_editor.as_mut() {
        editor.common.search_query = "song".to_string();
    }
    let before = editor_ids(&app);

    let _ = app.update(Message::Editor(EditorMessage::DragReorder(
        DragEvent::Dropped {
            index: 0,
            target_index: 2,
        },
    )));

    assert_eq!(
        editor_ids(&app),
        before,
        "reorder must be a no-op (guarded) while a search query is active"
    );
}

#[test]
fn editor_remove_under_active_search_maps_filtered_index() {
    // With a search active that filters the buffer down, RemoveAt receives an
    // index into the FILTERED view. It must map through the filtered view to
    // the correct full-buffer row — never delete by raw index. Buffer is
    // [a,b,c]; a search matching only "b" leaves the filtered view as [b], so
    // RemoveAt(0) must delete "b" (not "a").
    let mut app = test_app();
    app.playlist_editor = Some(PlaylistEditorState::new(PlaylistEditState::new(
        "pl_1".into(),
        "Test Playlist".into(),
        String::new(),
        false,
        Vec::new(),
    )));
    let _ = app.update(Message::Editor(EditorMessage::SongsLoaded(vec![
        make_queue_song("a", "Alpha", "Artist", "Album"),
        make_queue_song("b", "Bravo", "Artist", "Album"),
        make_queue_song("c", "Charlie", "Artist", "Album"),
    ])));

    if let Some(editor) = app.playlist_editor.as_mut() {
        editor.common.search_query = "bravo".to_string();
    }

    let _ = app.update(Message::Editor(EditorMessage::RemoveAt(0)));

    assert_eq!(
        editor_ids(&app),
        vec!["a", "c"],
        "RemoveAt under active search must map the filtered index to the right full-buffer row"
    );
}

// --- Phase 5: cross-pane drop targets the editor buffer ------------------

#[test]
fn drop_browser_song_inserts_into_editor_at_index() {
    // A completed browser→editor drop resolves to `SongsInserted { rows, at }`.
    // Buffer [a,b,c]; dropping a new row at slot 1 yields [a,NEW,b,c].
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::SongsInserted {
        rows: vec![make_queue_song("x", "Song X", "Artist", "Album")],
        at: 1,
    }));

    assert_eq!(
        editor_ids(&app),
        vec!["a", "x", "b", "c"],
        "a browser drop must splice the resolved row into the editor buffer at the drop slot"
    );

    // Fresh, unique entry_id: must not collide with any existing buffer row.
    let editor = app.playlist_editor.as_ref().expect("editor present");
    let inserted = editor
        .songs
        .iter()
        .find(|s| s.id == "x")
        .expect("inserted row present");
    let others: Vec<u64> = editor
        .songs
        .iter()
        .filter(|s| s.id != "x")
        .map(|s| s.entry_id)
        .collect();
    assert!(
        !others.contains(&inserted.entry_id),
        "inserted row must get a fresh entry_id that does not collide with existing rows"
    );
}

#[test]
fn drop_into_editor_does_not_modify_queue() {
    // Editing must keep the live queue byte-for-byte unchanged: a browser drop
    // splices into the editor buffer, never the queue.
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("q1", "Queue One", "Artist A", "Album A"),
        make_queue_song("q2", "Queue Two", "Artist B", "Album B"),
    ];
    let before = app.queue_song_ids();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::SongsInserted {
        rows: vec![make_queue_song("x", "Song X", "Artist", "Album")],
        at: 0,
    }));

    assert_eq!(
        app.queue_song_ids(),
        before,
        "a drop into the editor buffer must not mutate the live queue"
    );
}

#[test]
fn drop_appends_when_index_beyond_buffer() {
    // A drop slot past the buffer length clamps to the end (append) — the
    // editor-aware staleness gate / insert never panics on an out-of-range
    // index. Buffer [a,b,c]; dropping at slot 99 → appended at the tail.
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::SongsInserted {
        rows: vec![make_queue_song("x", "Song X", "Artist", "Album")],
        at: 99,
    }));

    assert_eq!(
        editor_ids(&app),
        vec!["a", "b", "c", "x"],
        "an out-of-range drop index must clamp to the buffer end (append)"
    );
}

#[test]
fn drop_staleness_gate_uses_editor_len() {
    // `compute_editor_drop_slot` is the editor-mode sibling of
    // `compute_queue_drop_slot`: it reads the EDITOR pane's hovered slot and
    // rejects payloads whose baked `items_len` no longer matches the EDITOR
    // buffer length — NOT the queue length. Construct queue_len != editor_len
    // so the test distinguishes which length the gate uses.
    use crate::widgets::HoveredSlot;

    let mut app = test_app();
    // Queue length 5 (deliberately different from the editor buffer).
    app.library.queue_songs = vec![
        make_queue_song("q1", "Q1", "ar", "Al"),
        make_queue_song("q2", "Q2", "ar", "Al"),
        make_queue_song("q3", "Q3", "ar", "Al"),
        make_queue_song("q4", "Q4", "ar", "Al"),
        make_queue_song("q5", "Q5", "ar", "Al"),
    ];
    seeded_editor(&mut app); // editor buffer len == 3

    // Hover payload baked against the editor's length (3) at item 1 → valid.
    if let Some(editor) = app.playlist_editor.as_mut() {
        editor.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
            slot_index: 1,
            item_index: 1,
            items_len: 3,
        });
    }
    assert_eq!(
        app.compute_editor_drop_slot(),
        Some(1),
        "a hover baked against the editor buffer length must resolve to its item index"
    );

    // Now bake against the QUEUE length (5). The editor gate must REJECT it as
    // stale, even though 5 matches the live queue length.
    if let Some(editor) = app.playlist_editor.as_mut() {
        editor.common.slot_list.hovered_slot = Some(HoveredSlot::Item {
            slot_index: 1,
            item_index: 1,
            items_len: 5,
        });
    }
    assert_eq!(
        app.compute_editor_drop_slot(),
        None,
        "the editor staleness gate must compare against the editor buffer length, not the queue"
    );
}

#[test]
fn drop_into_editor_makes_session_dirty() {
    // An insert changes membership, so the session dirties automatically
    // (dirty is computed from the buffer ids at render time).
    let mut app = test_app();
    seeded_editor(&mut app);
    assert!(
        !app.playlist_editor
            .as_ref()
            .unwrap()
            .edit
            .is_dirty(&app.editor_song_ids()),
        "freshly-loaded editor must be clean before the drop"
    );

    let _ = app.update(Message::Editor(EditorMessage::SongsInserted {
        rows: vec![make_queue_song("x", "Song X", "Artist", "Album")],
        at: 1,
    }));

    assert!(
        app.playlist_editor
            .as_ref()
            .unwrap()
            .edit
            .is_dirty(&app.editor_song_ids()),
        "inserting a row changes membership and must dirty the session"
    );
}

// --- Phase 6: save reads the editor buffer; exit needs no restore --------

#[test]
fn save_success_seeds_snapshot_from_editor_buffer() {
    // Dirty the editor (reorder), then dispatch the save-success message. The
    // snapshot must be re-seeded from the EDITOR buffer ids, so the session is
    // clean afterward. If save read the queue instead, the snapshot would
    // mismatch the editor buffer and the session would stay dirty — so a clean
    // session here is observable proof the save path uses the editor buffer.
    let mut app = test_app();
    seeded_editor(&mut app);

    let _ = app.update(Message::Editor(EditorMessage::DragReorder(
        DragEvent::Dropped {
            index: 0,
            target_index: 2,
        },
    )));
    assert!(
        app.playlist_editor
            .as_ref()
            .unwrap()
            .edit
            .is_dirty(&app.editor_song_ids()),
        "the reorder must leave the session dirty before save"
    );

    let _ = app.update(Message::SplitView(SplitViewMessage::PlaylistEditsSaved(
        String::new(),
    )));

    assert!(
        !app.playlist_editor
            .as_ref()
            .unwrap()
            .edit
            .is_dirty(&app.editor_song_ids()),
        "after save the snapshot must be re-seeded from the editor buffer, leaving it clean"
    );
}

#[test]
fn save_serializes_full_buffer_even_with_search() {
    // Preserve the verified-SAFE property: save serializes the FULL ordered
    // buffer, never the filtered subset. With a search active that filters the
    // buffer down, `editor_song_ids()` (what save serializes) must still return
    // ALL buffer ids in order.
    let mut app = test_app();
    seeded_editor(&mut app); // buffer [a,b,c]

    if let Some(editor) = app.playlist_editor.as_mut() {
        // A query matching only one row, so the filtered view is a strict subset.
        editor.common.search_query = "Song B".to_string();
    }

    // Sanity: the filtered view is indeed a subset.
    assert_eq!(
        app.filter_editor_songs().len(),
        1,
        "the search must filter the buffer down for this test to be meaningful"
    );

    assert_eq!(
        app.editor_song_ids(),
        vec!["a", "b", "c"],
        "save must serialize the full ordered buffer, not the filtered subset"
    );
}

#[test]
fn exit_leaves_queue_untouched() {
    // Seed a non-empty live queue, enter edit, load a DIFFERENT set into the
    // editor buffer, mutate it, then exit. The queue must be byte-for-byte
    // unchanged and the editor session cleared (no restore needed — the queue
    // was never touched: fixes bugs 3/11).
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("q1", "Queue One", "Artist A", "Album A"),
        make_queue_song("q2", "Queue Two", "Artist B", "Album B"),
    ];
    let before = app.queue_song_ids();

    seeded_editor(&mut app); // editor buffer is [a,b,c] — a different set
    let _ = app.update(Message::Editor(EditorMessage::RemoveAt(1)));

    let _ = app.update(Message::SplitView(SplitViewMessage::ExitEditMode));

    assert_eq!(
        app.queue_song_ids(),
        before,
        "exiting edit mode must leave the live queue untouched"
    );
    assert!(
        app.playlist_editor.is_none(),
        "exiting edit mode must clear the editor session"
    );
}

#[test]
fn exit_when_editor_dirty_warns() {
    // The discard warning must be driven by the editor's (correct) dirty state:
    // a mutated buffer at exit pushes the "Discarded unsaved playlist changes"
    // warn toast.
    let mut app = test_app();
    seeded_editor(&mut app);
    let _ = app.update(Message::Editor(EditorMessage::RemoveAt(1)));
    assert!(app.toast.toasts.is_empty(), "no toast before exit");

    let _ = app.update(Message::SplitView(SplitViewMessage::ExitEditMode));

    assert_eq!(
        app.toast.toasts.len(),
        1,
        "a dirty editor must push exactly one discard-warning toast on exit"
    );
    let toast = &app.toast.toasts[0];
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Warning);
    assert!(
        toast.message.to_lowercase().contains("discard"),
        "the toast must be the discard warning, got: {}",
        toast.message
    );
}

#[test]
fn exit_when_clean_no_warn() {
    // A clean editor at exit must NOT push the discard warning.
    let mut app = test_app();
    seeded_editor(&mut app); // freshly loaded == clean
    assert!(app.toast.toasts.is_empty(), "no toast before exit");

    let _ = app.update(Message::SplitView(SplitViewMessage::ExitEditMode));

    assert!(
        app.toast.toasts.is_empty(),
        "a clean editor must not push a discard warning on exit"
    );
}

// --- Phase 7: regression lock on root-cause-Y isolation ------------------
//
// These tests prove the editor buffer (`Nokkvi.playlist_editor.songs`) is
// STRUCTURALLY ISOLATED from the live play queue (`library.queue_songs`).
// Each sets up BOTH a populated queue AND an editor session loaded with a
// DIFFERENT set of rows, then drives a queue/playback mutation and asserts the
// editor buffer is byte-for-byte unchanged. The point is to lock in the
// decoupling so a future change cannot silently re-couple them (which is what
// caused the original root-cause-Y bugs 5/6/8/9).
//
// Bugs 8 (MPRIS / media keys) and 9 (scrobble during edit) are DISSOLVED BY
// CONSTRUCTION and asserted indirectly: the editor buffer is a separate `Vec`
// that is never the playback queue, and the scrobble/MPRIS handlers carry NO
// reference to `playlist_editor` at all (`src/update/scrobbling.rs`,
// `src/update/mpris.rs`, `src/update/playback.rs` — verified: zero matches for
// `playlist_editor`). They drive the engine/queue exclusively. There is no
// state-level `test_app` assertion for them because both require a live
// `app_service`/engine (scrobble issues an HTTP request; MPRIS reads engine
// transport), so they are listed as MANUAL smoke checks rather than faked. The
// queue-isolation tests below cover the buffer-mutation half of the same
// guarantee; the transport half is the manual check.

/// Snapshot the editor buffer's `(id, entry_id)` pairs for an exact
/// before/after equality assertion (catches reorder, removal, or insertion).
fn editor_buffer_snapshot(app: &crate::Nokkvi) -> Vec<(String, u64)> {
    app.playlist_editor
        .as_ref()
        .expect("editor session present")
        .songs
        .iter()
        .map(|s| (s.id.clone(), s.entry_id))
        .collect()
}

#[test]
fn consume_style_queue_mutation_does_not_alter_editor_buffer() {
    // Bug 5 regression: consume mode removes the just-played track from the
    // queue. With the old coupling, that removal hit the playlist-being-edited
    // and silently shrank the saved playlist. The remove-from-queue handler is
    // the same code path consume drives (`QueueAction::RemoveFromQueue` →
    // optimistic `library.queue_songs` mutation). Drive it with an editor
    // session present and assert the editor buffer is untouched.
    use crate::views::{QueueMessage, queue::QueueContextEntry};

    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("q1", "Queue One", "Artist A", "Album A"),
        make_queue_song("q2", "Queue Two", "Artist B", "Album B"),
        make_queue_song("q3", "Queue Three", "Artist C", "Album C"),
    ];
    seeded_editor(&mut app); // editor buffer is [a,b,c] — a DIFFERENT set
    let editor_before = editor_buffer_snapshot(&app);

    // Remove the head of the queue — exactly what a consume tick does after a
    // track finishes.
    let _ = app.handle_queue(QueueMessage::ContextMenuAction(
        0,
        QueueContextEntry::RemoveFromQueue,
    ));

    // The queue lost its head row...
    assert_eq!(
        app.queue_song_ids(),
        vec!["q2", "q3"],
        "the queue mutation must apply to the live queue"
    );
    // ...but the editor buffer is byte-for-byte unchanged.
    assert_eq!(
        editor_buffer_snapshot(&app),
        editor_before,
        "a consume-style queue removal must never touch the editor buffer (bug 5)"
    );
}

#[test]
fn queue_navigation_during_edit_leaves_editor_buffer_untouched() {
    // Bug 6 regression: auto-advance / reposition walks the queue cursor and
    // (via reorder) mutates queue order. The reachable, app_service-free queue
    // navigation/reorder path in test_app is the drag-reorder → MoveItem
    // optimistic local reorder of `library.queue_songs`. Drive a queue reorder
    // with an editor session present and assert the editor buffer is untouched.
    use crate::{views::QueueMessage, widgets::drag_column::DragEvent};

    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("q1", "Queue One", "Artist A", "Album A"),
        make_queue_song("q2", "Queue Two", "Artist B", "Album B"),
        make_queue_song("q3", "Queue Three", "Artist C", "Album C"),
    ];
    seeded_editor(&mut app); // editor buffer is [a,b,c]
    let editor_before = editor_buffer_snapshot(&app);

    // Reorder the queue (move row 0 down) — exercises the queue-navigation /
    // reorder mutation path on `library.queue_songs`.
    let _ = app.handle_queue(QueueMessage::DragReorder(DragEvent::Dropped {
        index: 0,
        target_index: 2,
    }));

    // The queue order changed...
    assert_eq!(
        app.queue_song_ids(),
        vec!["q2", "q1", "q3"],
        "the queue reorder must apply to the live queue"
    );
    // ...the editor buffer did not.
    assert_eq!(
        editor_buffer_snapshot(&app),
        editor_before,
        "queue navigation/reorder during edit must never touch the editor buffer (bug 6)"
    );
}

#[test]
fn scrobble_and_mpris_handlers_are_independent_of_editor_by_construction() {
    // Bugs 8 (MPRIS / media keys) and 9 (scrobble during edit) are dissolved by
    // construction: the editor buffer is a separate Vec, and the scrobble/MPRIS
    // handlers never read `playlist_editor`. This test documents that guarantee
    // at the level test_app CAN assert — that an editor session present does not
    // change the editor buffer when an unrelated playback-adjacent message flows
    // through update(). Full transport/scrobble behaviour requires a live engine
    // and is covered by the MANUAL smoke checks in the handoff.
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("q1", "Queue One", "Artist A", "Album A"),
        make_queue_song("q2", "Queue Two", "Artist B", "Album B"),
    ];
    seeded_editor(&mut app);
    let editor_before = editor_buffer_snapshot(&app);

    // A scrobble-submission result (the path that increments play counts) flows
    // through update(); it reads queue/engine state, never the editor buffer.
    let _ = app.update(Message::Scrobble(
        crate::app_message::ScrobbleMessage::SubmissionResult(Ok("q1".to_string())),
    ));

    assert_eq!(
        editor_buffer_snapshot(&app),
        editor_before,
        "scrobble/MPRIS-adjacent messages must never touch the editor buffer (bugs 8/9)"
    );
}

// --- Phase 8: CRUD-from-queue-banner fixes --------------------------------
//
// These pin the save/enter behavior reachable from the queue "Playing From"
// banner's edit button (QueueAction::EditPlaylist), the exact path the user hit.

/// A library-list playlist entry with explicit name/comment/updated_at, for
/// seeding `library.playlists` (the post-save cache the banner-refresh reads).
fn playlist_entry(
    id: &str,
    name: &str,
    comment: &str,
    updated_at: &str,
) -> nokkvi_data::backend::playlists::PlaylistUIViewData {
    nokkvi_data::backend::playlists::PlaylistUIViewData {
        id: id.into(),
        name: name.into(),
        comment: comment.into(),
        duration: 0.0,
        song_count: 0,
        owner_name: String::new(),
        public: true,
        updated_at: updated_at.into(),
        artwork_album_ids: vec![],
        searchable_lower: name.to_lowercase(),
    }
}

/// Seed a loaded editor session for a specific playlist id/name/comment so the
/// save handler's `id`/name/comment line up with a chosen `active_playlist_info`.
fn seeded_editor_for(app: &mut crate::Nokkvi, id: &str, name: &str, comment: &str) {
    app.playlist_editor = Some(PlaylistEditorState::new(PlaylistEditState::new(
        id.into(),
        name.into(),
        comment.into(),
        true,
        Vec::new(),
    )));
    let _ = app.update(Message::Editor(EditorMessage::SongsLoaded(vec![
        make_queue_song("a", "Song A", "Artist", "Album"),
        make_queue_song("b", "Song B", "Artist", "Album"),
    ])));
}

#[test]
fn save_reflects_typed_name_in_banner_over_stale_cache() {
    // Bug: playing FROM a playlist, edit it from the queue banner, RENAME, Save.
    // The banner must show the just-typed name. The save handler rebuilds
    // active_playlist_info from library.playlists, which still holds the OLD
    // name (the reload is async and hasn't run), so the refresh must NOT let the
    // stale cache entry clobber the name the user just typed and saved.
    let mut app = test_app();
    app.library.playlists.append_page(
        vec![playlist_entry("pl_1", "Old Name", "Old comment", "T0")],
        1,
    );
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "pl_1".into(),
        "Old Name".into(),
        "Old comment".into(),
    ));
    seeded_editor_for(&mut app, "pl_1", "Old Name", "Old comment");

    let _ = app.update(Message::Editor(EditorMessage::NameChanged(
        "New Name".into(),
    )));
    let _ = app.update(Message::SplitView(SplitViewMessage::PlaylistEditsSaved(
        String::new(),
    )));

    assert_eq!(
        app.active_playlist_info.as_ref().map(|c| c.name.as_str()),
        Some("New Name"),
        "the Playing-From banner must show the just-saved name, not the stale library-cache value"
    );
}

#[test]
fn save_reflects_typed_comment_in_banner_over_stale_cache() {
    // Same bug, comment field: editing the comment and saving must surface the
    // new comment in the banner, not snap back to the stale cached comment.
    let mut app = test_app();
    app.library.playlists.append_page(
        vec![playlist_entry("pl_1", "Old Name", "Old comment", "T0")],
        1,
    );
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "pl_1".into(),
        "Old Name".into(),
        "Old comment".into(),
    ));
    seeded_editor_for(&mut app, "pl_1", "Old Name", "Old comment");

    let _ = app.update(Message::Editor(EditorMessage::CommentChanged(
        "New comment".into(),
    )));
    let _ = app.update(Message::SplitView(SplitViewMessage::PlaylistEditsSaved(
        String::new(),
    )));

    assert_eq!(
        app.active_playlist_info
            .as_ref()
            .map(|c| c.comment.as_str()),
        Some("New comment"),
        "the Playing-From banner must show the just-saved comment, not the stale cache value"
    );
}

#[test]
fn save_advances_loaded_updated_at_token() {
    // A successful save must advance the editor's optimistic-concurrency token
    // to the server's new updatedAt (carried on PlaylistEditsSaved). Otherwise a
    // SECOND track-changing save in the same still-mounted session re-reads a
    // server token the FIRST save already bumped and false-aborts as "stale".
    let mut app = test_app();
    seeded_editor_for(&mut app, "pl_1", "Mix", "");
    if let Some(editor) = app.playlist_editor.as_mut() {
        editor.edit.set_loaded_updated_at("T0".into());
    }

    let _ = app.update(Message::SplitView(SplitViewMessage::PlaylistEditsSaved(
        "T1".into(),
    )));

    assert_eq!(
        app.playlist_editor
            .as_ref()
            .expect("editor session present")
            .edit
            .loaded_updated_at(),
        "T1",
        "a successful save must advance the staleness token to the server's new value"
    );
}

#[test]
fn enter_edit_seeds_staleness_token_from_active_context_when_list_empty() {
    // Banner-edit entry with the playlists list NOT loaded (e.g. a restored
    // session that opened on Queue): the optimistic-concurrency token must fall
    // back to the active-playlist context (same object the name/comment come
    // from) instead of staying empty, which would silently disable the guard.
    let mut app = test_app();
    // library.playlists is empty; the active context carries the server token.
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::from_persisted(
        "pl_1".into(),
        "Mix".into(),
        String::new(),
        0.0,
        "SERVER_TOKEN".into(),
        true,
        3,
    ));

    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "pl_1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    }));

    assert_eq!(
        app.playlist_editor
            .as_ref()
            .expect("editor session present")
            .edit
            .loaded_updated_at(),
        "SERVER_TOKEN",
        "with the playlists list empty, the staleness token must fall back to the active context"
    );
}
