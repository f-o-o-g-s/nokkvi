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
