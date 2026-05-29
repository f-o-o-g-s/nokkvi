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
