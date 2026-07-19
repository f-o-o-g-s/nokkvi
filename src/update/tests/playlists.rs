//! M1 smart-awareness safety-layer handler tests.
//!
//! Every write path that would 403 against a smart playlist must refuse
//! politely BEFORE any editor mounts or network fires: the EditPlaylist
//! action, the queue-banner pencil, and the quick-add default-playlist
//! bypass. Driven through the full dispatcher (`app.update`) so each test
//! covers the real routing chain, asserting observable state only (editor
//! mount, dialog visibility, toast level/copy).

use nokkvi_data::types::toast::ToastLevel;

use crate::{
    Message,
    test_helpers::test_app,
    views::{self, playlists::PlaylistContextEntry},
};

/// A library-list playlist entry with controllable smartness/ownership.
fn playlist_row(
    id: &str,
    name: &str,
    smart: bool,
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
        is_smart: smart,
        rules: smart
            .then(|| serde_json::json!({ "all": [ { "is": { "loved": true } } ], "limit": 10 })),
        evaluated_at: smart.then(|| "2026-07-01T10:00:00Z".into()),
        is_file_backed: false,
        sync: false,
        owner_id: "user-9".into(),
        searchable_lower: name.to_lowercase(),
    }
}

fn seed_playlists(app: &mut crate::Nokkvi, rows: Vec<(&str, &str, bool)>) {
    let entries: Vec<_> = rows
        .into_iter()
        .map(|(id, name, smart)| playlist_row(id, name, smart))
        .collect();
    let count = entries.len();
    app.library.playlists.append_page(entries, count);
}

fn last_toast(app: &crate::Nokkvi) -> Option<(ToastLevel, String)> {
    app.toast
        .toasts
        .back()
        .map(|t| (t.level, t.message.clone()))
}

// --- EditPlaylist smart gate (dispatch site 1) ----------------------------

/// The Playlists-view EditPlaylist action on a smart row must never mount
/// the TRACKS editor — since M4 it routes into the RULES session instead
/// (the M1 interim toast was the placeholder for exactly this routing).
/// Without caps the refusal is the version toast; with caps + ownership
/// the rules session mounts.
#[test]
fn edit_playlist_on_smart_row_routes_to_rules() {
    // Caps unknown: refused with the version toast, nothing mounts.
    let mut app = test_app();
    seed_playlists(&mut app, vec![("sp1", "Never Played", true)]);
    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::EditPlaylist),
    ));
    assert!(app.playlist_editor.is_none());
    assert_eq!(
        last_toast(&app).map(|(l, _)| l),
        Some(ToastLevel::Warning),
        "caps-unknown refusal is a warning"
    );

    // Caps fetched + owned: the RULES session mounts (never Tracks).
    let mut app = test_app();
    app.caps_state = crate::state::CapsState::Fetched(
        nokkvi_data::types::smart_criteria::ServerCaps::from_version_str("0.63.2"),
    );
    app.session_user_id = "user-9".into();
    seed_playlists(&mut app, vec![("sp1", "Never Played", true)]);
    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::EditPlaylist),
    ));
    assert!(
        app.rules_session().is_some(),
        "a smart row's edit routes into the rules session"
    );
}

/// The same action on a regular row passes the gate silently (no refusal
/// toast), and the chained EnterEditMode still mounts the editor — the gate
/// must not over-block. (The mount rides a `Task::done` the test harness
/// doesn't execute, so the chained message is driven directly.)
#[test]
fn edit_playlist_on_regular_row_still_mounts_editor() {
    let mut app = test_app();
    seed_playlists(&mut app, vec![("pl1", "Road Trip", false)]);

    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::EditPlaylist),
    ));
    assert!(
        app.toast.toasts.is_empty(),
        "the smart gate must stay silent on a regular row"
    );

    let _ = app.update(Message::SplitView(
        crate::app_message::SplitViewMessage::EnterEditMode {
            playlist_id: "pl1".into(),
            playlist_name: "Road Trip".into(),
            playlist_comment: String::new(),
            playlist_public: true,
        },
    ));
    assert!(
        app.playlist_editor.is_some(),
        "a regular playlist must still open the Tracks editor"
    );
}

/// The central backstop: even a direct EnterEditMode dispatch (the chained
/// create path) refuses a smart id.
#[test]
fn enter_edit_mode_backstop_refuses_smart_id() {
    let mut app = test_app();
    seed_playlists(&mut app, vec![("sp1", "Never Played", true)]);

    let _ = app.update(Message::SplitView(
        crate::app_message::SplitViewMessage::EnterEditMode {
            playlist_id: "sp1".into(),
            playlist_name: "Never Played".into(),
            playlist_comment: String::new(),
            playlist_public: true,
        },
    ));

    assert!(
        app.playlist_editor.is_none(),
        "the EnterEditMode backstop must refuse a smart id"
    );
    assert_eq!(
        last_toast(&app).map(|(l, _)| l),
        Some(ToastLevel::Warning),
        "the backstop refusal surfaces a warn toast"
    );
}

// --- Queue-banner pencil (dispatch site 2) --------------------------------

/// Banner pencil with a KNOWN-smart active playlist (fresh library hit):
/// since M4, routes into the RULES session — a Tracks editor never mounts
/// on a smart playlist.
#[test]
fn banner_pencil_on_known_smart_routes_to_rules() {
    let mut app = test_app();
    app.caps_state = crate::state::CapsState::Fetched(
        nokkvi_data::types::smart_criteria::ServerCaps::from_version_str("0.63.2"),
    );
    app.session_user_id = "user-9".into();
    seed_playlists(&mut app, vec![("sp1", "Never Played", true)]);
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "sp1".into(),
        "Never Played".into(),
        String::new(),
    ));

    let _ = app.update(Message::Queue(views::QueueMessage::EditPlaylist));

    assert!(
        app.rules_session().is_some(),
        "the pencil on a known-smart playlist lands in the rules session"
    );
}

/// Banner pencil with `smart: None` and no library hit (Harbour-played /
/// restored session): the M4 just-in-time meta fetch disambiguates BEFORE
/// routing — nothing mounts synchronously and no refusal toast fires (the
/// M1 interim refusal was replaced by this lane).
#[test]
fn banner_pencil_with_unknown_smartness_fetches_before_routing() {
    let mut app = test_app();
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "mystery".into(),
        "Played From Harbour".into(),
        String::new(),
    ));

    let _ = app.update(Message::Queue(views::QueueMessage::EditPlaylist));

    assert!(
        app.playlist_editor.is_none(),
        "unknown smartness must never mount ANY editor synchronously — the \
         JIT fetch resolves the kind first (no late 403s)"
    );
    assert!(
        app.toast.toasts.is_empty(),
        "the fetch lane refuses nothing up front"
    );
}

/// Banner pencil with a known-REGULAR playlist keeps working (fresh library
/// hit says regular even though the context itself is minimal): no refusal
/// toast fires, and the chained EnterEditMode mounts.
#[test]
fn banner_pencil_on_known_regular_still_opens_editor() {
    let mut app = test_app();
    seed_playlists(&mut app, vec![("pl1", "Road Trip", false)]);
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "pl1".into(),
        "Road Trip".into(),
        String::new(),
    ));

    let _ = app.update(Message::Queue(views::QueueMessage::EditPlaylist));
    assert!(
        app.toast.toasts.is_empty(),
        "a known-regular pencil press must not toast a refusal"
    );

    let _ = app.update(Message::SplitView(
        crate::app_message::SplitViewMessage::EnterEditMode {
            playlist_id: "pl1".into(),
            playlist_name: "Road Trip".into(),
            playlist_comment: String::new(),
            playlist_public: true,
        },
    ));
    assert!(
        app.playlist_editor.is_some(),
        "a known-regular active playlist must still open the editor"
    );
}

// --- Quick-add default-playlist bypass ------------------------------------

/// Quick-add with a SMART default playlist: the bypass refuses with the
/// smart-specific toast and falls through to the (smart-filtered) dialog.
#[test]
fn quick_add_smart_default_toasts_and_opens_dialog() {
    let mut app = test_app();
    app.settings.quick_add_to_playlist = true;
    app.settings.default_playlist_id = Some("sp1".into());
    app.settings.default_playlist_name = "Never Played".into();

    let triples = vec![
        ("sp1".to_owned(), "Never Played".to_owned(), true),
        ("pl1".to_owned(), "Road Trip".to_owned(), false),
    ];
    let _ = app.update(Message::PlaylistsFetchedForAddToPlaylist(
        triples,
        vec!["song1".into()],
    ));

    let (level, msg) = last_toast(&app).expect("smart-default toast expected");
    assert_eq!(level, ToastLevel::Warning);
    assert!(
        msg.contains("smart"),
        "the smart case must be labeled as smart, got: {msg}"
    );
    assert!(
        app.text_input_dialog.visible,
        "the picker dialog must open so the add can still complete"
    );
}

/// Quick-add whose default id is ABSENT from the fetched list: the
/// distinguishable "unavailable" toast (never mislabeled as smart) + dialog.
#[test]
fn quick_add_missing_default_toasts_unavailable() {
    let mut app = test_app();
    app.settings.quick_add_to_playlist = true;
    app.settings.default_playlist_id = Some("gone".into());
    app.settings.default_playlist_name = "Deleted One".into();

    let triples = vec![("pl1".to_owned(), "Road Trip".to_owned(), false)];
    let _ = app.update(Message::PlaylistsFetchedForAddToPlaylist(
        triples,
        vec!["song1".into()],
    ));

    let (level, msg) = last_toast(&app).expect("unavailable toast expected");
    assert_eq!(level, ToastLevel::Warning);
    assert!(
        msg.contains("unavailable"),
        "the missing case must say unavailable, got: {msg}"
    );
    assert!(
        !msg.contains("smart"),
        "the missing case must never be mislabeled as smart, got: {msg}"
    );
    assert!(app.text_input_dialog.visible);
}

// --- File-backed honesty copy ---------------------------------------------

/// Deleting a file-backed playlist extends the confirm with the honest
/// resurrect note; a regular playlist's confirm stays note-free.
#[test]
fn delete_confirm_notes_file_backed_resurrection() {
    let mut app = test_app();
    let mut file_backed = playlist_row("fb1", "Movie Soundtracks", true);
    file_backed.is_file_backed = true;
    file_backed.sync = true;
    app.library.playlists.append_page(vec![file_backed], 1);

    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::Delete),
    ));
    assert!(app.text_input_dialog.visible);
    let note = app
        .text_input_dialog
        .note
        .as_deref()
        .expect("file-backed delete must carry the resurrect note");
    assert!(note.contains("return on the next scan"), "got: {note}");

    // Regular playlist: no note.
    let mut app = test_app();
    seed_playlists(&mut app, vec![("pl1", "Road Trip", false)]);
    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::Delete),
    ));
    assert!(app.text_input_dialog.visible);
    assert_eq!(
        app.text_input_dialog.note, None,
        "a regular delete confirm carries no file note"
    );
}

/// Renaming a synced file-backed playlist carries the truthful residual
/// note (rules re-sync) — and NEVER a false resurrection warning.
#[test]
fn rename_dialog_notes_rules_resync_for_synced_file() {
    let mut app = test_app();
    let mut file_backed = playlist_row("fb1", "Movie Soundtracks", true);
    file_backed.is_file_backed = true;
    file_backed.sync = true;
    app.library.playlists.append_page(vec![file_backed], 1);

    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::Rename),
    ));
    assert!(app.text_input_dialog.visible);
    let note = app
        .text_input_dialog
        .note
        .as_deref()
        .expect("synced file-backed rename must carry the residual note");
    assert!(
        note.contains("rules keep overwriting"),
        "the note states the TRUE residual (rules re-sync), got: {note}"
    );
    assert!(
        !note.contains("return"),
        "rename must never claim resurrection — scan re-sync preserves the API-set name"
    );

    // A detached (sync=false) file-backed playlist renames without the note.
    let mut app = test_app();
    let mut detached = playlist_row("fb2", "Detached", true);
    detached.is_file_backed = true;
    detached.sync = false;
    app.library.playlists.append_page(vec![detached], 1);
    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::PlaylistContextAction(0, PlaylistContextEntry::Rename),
    ));
    assert_eq!(app.text_input_dialog.note, None);
}

// --- Editor comment reserved-prefix strip ---------------------------------

/// Typing a comment that begins with the draft-marker prefix strips it and
/// surfaces the reserved-prefix diagnostic; prose mentions elsewhere in the
/// comment are untouched.
#[test]
fn editor_comment_strips_reserved_prefix() {
    let mut app = test_app();
    app.playlist_editor = Some(crate::state::PlaylistEditorState::new(
        nokkvi_data::types::playlist_edit::PlaylistEditState::new(
            "pl1".into(),
            "Road Trip".into(),
            String::new(),
            true,
            Vec::new(),
        ),
    ));

    let _ = app.update(Message::Editor(
        crate::app_message::EditorMessage::CommentChanged("nokkvi-draft/1 pid=1 ts=2".into()),
    ));
    let editor = app.playlist_editor.as_ref().expect("editor stays mounted");
    assert_eq!(
        editor.edit.playlist_comment, "1 pid=1 ts=2",
        "the reserved prefix must be stripped from the stored comment"
    );
    let (level, msg) = last_toast(&app).expect("reserved-prefix diagnostic expected");
    assert_eq!(level, ToastLevel::Info);
    assert!(msg.contains("reserved"), "got: {msg}");

    // A prose mention that does not LEAD the comment is untouched.
    let _ = app.update(Message::Editor(
        crate::app_message::EditorMessage::CommentChanged("about nokkvi-draft/ markers".into()),
    ));
    let editor = app.playlist_editor.as_ref().expect("editor stays mounted");
    assert_eq!(editor.edit.playlist_comment, "about nokkvi-draft/ markers");
}

// --- M2b/c: expansion child context menus + remove-from-playlist -----------

use crate::{test_helpers::make_song, widgets::context_menu::LibraryContextEntry};

/// Seed an expanded playlist (index 0 in the library) with three children.
/// Flattened rows: [parent pl, child s1, child s2, child s3].
fn seed_expanded_playlist(app: &mut crate::Nokkvi, smart: bool) {
    seed_playlists(app, vec![("pl1", "Road Trip", smart)]);
    app.playlists_page.expansion.expanded_id = Some("pl1".into());
    app.playlists_page.expansion.children = vec![
        make_song("s1", "Song One", "Artist"),
        make_song("s2", "Song Two", "Artist"),
        make_song("s3", "Song Three", "Artist"),
    ];
}

/// Child AddToPlaylist resolves the batch and returns AddBatchToPlaylist —
/// the arm that shipped as a rendered no-op before M2.
#[test]
fn child_add_to_playlist_returns_batch_action() {
    let mut app = test_app();
    seed_expanded_playlist(&mut app, false);
    let playlists: Vec<_> = app.library.playlists.iter().cloned().collect();

    let (_task, action) = app.playlists_page.update(
        views::PlaylistsMessage::ContextMenuAction(1, LibraryContextEntry::AddToPlaylist),
        playlists.len(),
        &playlists,
    );

    match action {
        views::PlaylistsAction::AddBatchToPlaylist(payload) => {
            assert_eq!(payload.items.len(), 1, "one clicked child → one batch item");
        }
        other => panic!("expected AddBatchToPlaylist, got {other:?}"),
    }
}

/// Child AddToMix seeds the Trawl crate through the full dispatcher —
/// the playlists CHILD lane (the parent lane is pinned in trawl_menu.rs).
#[test]
fn child_add_to_mix_seeds_crate() {
    let mut app = test_app();
    seed_expanded_playlist(&mut app, false);

    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::ContextMenuAction(2, LibraryContextEntry::AddToMix),
    ));

    let keys: Vec<_> = app
        .trawl_crate
        .seeds
        .iter()
        .map(|s| s.key().1.to_string())
        .collect();
    assert_eq!(keys, vec!["s2"], "the clicked child must seed the crate");
}

/// RemoveFromPlaylist resolves the parent id + the 1-based ORDINAL (never
/// the song id — duplicates make ids ambiguous) + the verify-read token.
#[test]
fn child_remove_from_playlist_resolves_ordinal_position() {
    let mut app = test_app();
    seed_expanded_playlist(&mut app, false);
    let playlists: Vec<_> = app.library.playlists.iter().cloned().collect();

    let (_task, action) = app.playlists_page.update(
        views::PlaylistsMessage::ContextMenuAction(3, LibraryContextEntry::RemoveFromPlaylist),
        playlists.len(),
        &playlists,
    );

    match action {
        views::PlaylistsAction::RemoveTrackFromPlaylist {
            playlist_id,
            song_id,
            position,
        } => {
            assert_eq!(playlist_id, "pl1");
            assert_eq!(song_id, "s3", "the verify-read token is the clicked song");
            assert_eq!(
                position, 3,
                "flattened row 3 = child ordinal 2 = wire position 3"
            );
        }
        other => panic!("expected RemoveTrackFromPlaylist, got {other:?}"),
    }
}

/// The settle lane toasts by outcome: removed ⇒ success; changed ⇒ the
/// "refresh and retry" warning. Both refresh (task shape not asserted).
#[test]
fn track_removal_settled_toasts_by_outcome() {
    let mut app = test_app();
    seed_expanded_playlist(&mut app, false);
    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::TrackRemovalSettled {
            playlist_id: "pl1".into(),
            removed: true,
        },
    ));
    assert_eq!(
        last_toast(&app).map(|(l, _)| l),
        Some(ToastLevel::Success),
        "a confirmed removal toasts success"
    );

    let mut app = test_app();
    seed_expanded_playlist(&mut app, false);
    let _ = app.update(Message::Playlists(
        views::PlaylistsMessage::TrackRemovalSettled {
            playlist_id: "pl1".into(),
            removed: false,
        },
    ));
    let (level, msg) = last_toast(&app).expect("changed lane must toast");
    assert_eq!(level, ToastLevel::Warning);
    assert!(msg.contains("refresh and retry"), "got: {msg}");
}

/// `playlist_child_entries` hides the destructive entry for smart parents
/// and keeps the shared entries in both forms.
#[test]
fn playlist_child_entries_gate_remove_on_smart_parent() {
    use crate::widgets::context_menu::playlist_child_entries;
    let regular = playlist_child_entries(false);
    assert!(
        regular
            .iter()
            .any(|e| matches!(e, LibraryContextEntry::RemoveFromPlaylist)),
        "regular parents offer Remove from Playlist"
    );
    let smart = playlist_child_entries(true);
    assert!(
        !smart
            .iter()
            .any(|e| matches!(e, LibraryContextEntry::RemoveFromPlaylist)),
        "smart parents must never offer Remove from Playlist (server rejects it)"
    );
    for entries in [&regular, &smart] {
        for required in [
            entries
                .iter()
                .any(|e| matches!(e, LibraryContextEntry::ShufflePlay)),
            entries
                .iter()
                .any(|e| matches!(e, LibraryContextEntry::AddToQueue)),
            entries
                .iter()
                .any(|e| matches!(e, LibraryContextEntry::AddToPlaylist)),
            entries
                .iter()
                .any(|e| matches!(e, LibraryContextEntry::AddToMix)),
            entries
                .iter()
                .any(|e| matches!(e, LibraryContextEntry::GetInfo)),
        ] {
            assert!(required, "shared child entries must survive in both forms");
        }
    }
}

// --- M2d: duplicate-name warning on create dialogs -------------------------

/// Typing an existing name (any case, padded) into a playlist-create dialog
/// surfaces the dimmed duplicate note; a fresh name clears it. Warn-only —
/// submission stays possible (a duplicate name is legal server-side).
#[test]
fn create_dialog_warns_on_duplicate_name() {
    let mut app = test_app();
    seed_playlists(&mut app, vec![("pl1", "Road Trip", false)]);
    app.text_input_dialog.open(
        "New Playlist",
        "",
        "Playlist name...",
        crate::widgets::text_input_dialog::TextInputDialogAction::CreatePlaylistFromQueue,
    );

    let _ = app.update(Message::TextInputDialog(
        crate::widgets::text_input_dialog::TextInputDialogMessage::ValueChanged(
            "  road trip ".into(),
        ),
    ));
    let note = app
        .text_input_dialog
        .note
        .as_deref()
        .expect("duplicate name must surface the note");
    assert!(note.contains("Road Trip"), "got: {note}");

    let _ = app.update(Message::TextInputDialog(
        crate::widgets::text_input_dialog::TextInputDialogMessage::ValueChanged(
            "Fresh Name".into(),
        ),
    ));
    assert_eq!(
        app.text_input_dialog.note, None,
        "a fresh name clears the warning"
    );
}
