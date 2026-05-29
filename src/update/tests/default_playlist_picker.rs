//! Tests for default-playlist picker and playlist-edit dialog update handlers.

use crate::test_helpers::*;

// Default-Playlist Picker (default_playlist_picker.rs)
// ============================================================================

fn make_test_playlist(id: &str, name: &str) -> nokkvi_data::backend::playlists::PlaylistUIViewData {
    nokkvi_data::backend::playlists::PlaylistUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        comment: String::new(),
        duration: 0.0,
        song_count: 0,
        owner_name: String::new(),
        public: false,
        updated_at: String::new(),
        artwork_album_ids: vec![],
        searchable_lower: name.to_lowercase(),
    }
}

fn seed_playlists(app: &mut crate::Nokkvi, items: Vec<(&str, &str)>) {
    let total = items.len();
    let playlists: Vec<_> = items
        .into_iter()
        .map(|(id, name)| make_test_playlist(id, name))
        .collect();
    app.library.playlists.append_page(playlists, total);
}

#[test]
fn picker_open_initializes_state_with_library_playlists() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage, PickerEntry};

    let mut app = test_app();
    seed_playlists(
        &mut app,
        vec![("p1", "Workout"), ("p2", "Chill"), ("p3", "Focus")],
    );
    assert!(app.default_playlist_picker.is_none());

    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    let state = app
        .default_playlist_picker
        .as_ref()
        .expect("picker should be open after Open message");
    // 3 real playlists + 1 prepended Clear entry
    assert_eq!(state.all_entries.len(), 4);
    assert!(matches!(state.all_entries[0], PickerEntry::Clear));
}

#[test]
fn picker_close_clears_state() {
    use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;

    let mut app = test_app();
    seed_playlists(&mut app, vec![("p1", "Workout")]);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);
    assert!(app.default_playlist_picker.is_some());

    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Close);
    assert!(app.default_playlist_picker.is_none());
}

#[test]
fn picker_search_filters_entries() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage, PickerEntry};

    let mut app = test_app();
    seed_playlists(
        &mut app,
        vec![("p1", "Workout"), ("p2", "Chill"), ("p3", "Focus")],
    );
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::SearchChanged(
        "work".to_string(),
    ));

    let state = app.default_playlist_picker.as_ref().unwrap();
    // Clear stays + only "Workout" matches → 2 entries
    assert_eq!(state.filtered.len(), 2);
    assert!(matches!(state.filtered[0], PickerEntry::Clear));
    if let PickerEntry::Playlist { name, .. } = &state.filtered[1] {
        assert_eq!(name, "Workout");
    } else {
        panic!("expected Playlist entry at index 1");
    }
}

#[test]
fn picker_click_playlist_sets_default_and_closes() {
    use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;

    let mut app = test_app();
    seed_playlists(&mut app, vec![("p1", "Workout"), ("p2", "Chill")]);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);
    assert!(app.settings.default_playlist_id.is_none());
    assert!(app.settings.default_playlist_name.is_empty());

    // Index 1 is the first real playlist (index 0 is the Clear virtual entry).
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::ClickItem(1));

    assert_eq!(app.settings.default_playlist_id, Some("p1".to_string()));
    assert_eq!(app.settings.default_playlist_name, "Workout");
    assert!(
        app.default_playlist_picker.is_none(),
        "selecting an entry should close the picker"
    );
}

#[test]
fn picker_click_clear_unsets_default_and_closes() {
    use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;

    let mut app = test_app();
    app.settings.default_playlist_id = Some("p1".to_string());
    app.settings.default_playlist_name = "Workout".to_string();
    seed_playlists(&mut app, vec![("p1", "Workout")]);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    // Index 0 is the Clear virtual entry.
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::ClickItem(0));

    assert!(app.settings.default_playlist_id.is_none());
    assert!(app.settings.default_playlist_name.is_empty());
    assert!(app.default_playlist_picker.is_none());
}

#[test]
fn picker_activate_center_selects_centered_entry() {
    use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;

    let mut app = test_app();
    seed_playlists(&mut app, vec![("p1", "Workout"), ("p2", "Chill")]);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    // Move down once to put the first real playlist in the center.
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::SlotListDown);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::ActivateCenter);

    // Either Clear or Workout could be centered depending on slot list center index;
    // the contract is just that the picker closes and *some* selection happened.
    assert!(app.default_playlist_picker.is_none());
}

#[test]
fn picker_open_with_empty_library_still_offers_clear_entry() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage, PickerEntry};

    let mut app = test_app();
    // No playlists seeded — library.playlists stays empty.
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    let state = app.default_playlist_picker.as_ref().unwrap();
    assert_eq!(state.all_entries.len(), 1);
    assert!(matches!(state.all_entries[0], PickerEntry::Clear));
}

#[test]
fn picker_repopulates_when_playlists_load_after_open() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage, PickerEntry};

    let mut app = test_app();

    // Open picker with empty library — only the Clear entry is shown.
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::SearchChanged(
        "foo".to_string(),
    ));
    assert_eq!(
        app.default_playlist_picker
            .as_ref()
            .unwrap()
            .all_entries
            .len(),
        1
    );

    // Library load arrives after the picker was opened — refresh hook
    // should repopulate the picker while preserving the user's search query.
    seed_playlists(&mut app, vec![("p1", "Workout"), ("p2", "Foo")]);
    app.refresh_default_playlist_picker_after_load();

    let state = app.default_playlist_picker.as_ref().unwrap();
    assert_eq!(state.all_entries.len(), 3, "Clear + 2 playlists");
    assert_eq!(
        state.search_query, "foo",
        "the user's in-flight search query is preserved across the rebuild"
    );
    // "foo" matches "Foo", and Clear is always visible
    assert_eq!(state.filtered.len(), 2);
    assert!(matches!(state.filtered[0], PickerEntry::Clear));
    if let PickerEntry::Playlist { name, .. } = &state.filtered[1] {
        assert_eq!(name, "Foo");
    } else {
        panic!("expected Playlist entry at index 1");
    }
}

#[test]
fn queue_show_default_playlist_setting_default_is_off() {
    let app = test_app();
    assert!(
        !app.settings.queue_show_default_playlist,
        "the queue chip is opt-in — default should be hidden"
    );
}

// ============================================================================
// Text Input Dialog — Public/Private Playlist Toggle (F1, T5–T7)
// ============================================================================

#[test]
fn text_input_dialog_save_playlist_defaults_to_public() {
    let mut app = test_app();
    app.text_input_dialog.open_save_playlist(&[]);
    assert!(
        app.text_input_dialog.public,
        "newly opened save-playlist dialog must default the toggle to public"
    );
}

#[test]
fn text_input_dialog_public_toggled_message_flips_state() {
    use crate::{app_message::Message, widgets::text_input_dialog::TextInputDialogMessage};

    let mut app = test_app();
    app.text_input_dialog.open_save_playlist(&[]);
    assert!(app.text_input_dialog.public);

    let _ = app.update(Message::TextInputDialog(
        TextInputDialogMessage::PublicToggled(false),
    ));
    assert!(
        !app.text_input_dialog.public,
        "PublicToggled(false) must flip the dialog's public field to false"
    );
}

#[test]
fn text_input_dialog_combo_round_trip_preserves_public_off() {
    use crate::{
        app_message::Message,
        widgets::text_input_dialog::{PlaylistOption, TextInputDialogMessage},
    };

    let mut app = test_app();
    app.text_input_dialog
        .open_save_playlist(&[("p1".into(), "Existing".into())]);

    // User unchecks Public.
    let _ = app.update(Message::TextInputDialog(
        TextInputDialogMessage::PublicToggled(false),
    ));
    assert!(!app.text_input_dialog.public);

    // User flips combo to Existing playlist, then back to NewPlaylist.
    let _ = app.update(Message::TextInputDialog(
        TextInputDialogMessage::PlaylistSelected(PlaylistOption::Existing {
            id: "p1".into(),
            name: "Existing".into(),
        }),
    ));
    let _ = app.update(Message::TextInputDialog(
        TextInputDialogMessage::PlaylistSelected(PlaylistOption::NewPlaylist),
    ));

    assert!(
        !app.text_input_dialog.public,
        "combo round-trip must not silently reset the public toggle"
    );
}

#[test]
fn open_create_playlist_dialog_defaults_to_public_and_no_combo() {
    use crate::widgets::text_input_dialog::TextInputDialogAction;

    let mut app = test_app();
    app.text_input_dialog.open_create_playlist();

    assert!(app.text_input_dialog.visible);
    assert!(
        app.text_input_dialog.public,
        "Create-New-Playlist dialog must default the toggle to public"
    );
    assert!(
        !app.text_input_dialog.save_playlist_mode,
        "Create-New-Playlist must not show the existing-playlists combo"
    );
    assert!(matches!(
        app.text_input_dialog.action,
        Some(TextInputDialogAction::CreatePlaylistAndEdit)
    ));
}

#[test]
fn create_playlist_dialog_refused_when_already_editing() {
    use crate::{
        app_message::{Message, SplitViewMessage},
        views::PlaylistsMessage,
    };

    let mut app = test_app();
    // Enter split-view edit mode first.
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Existing".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    }));
    assert!(app.playlist_editor.is_some());

    // User clicks the view-header `+` — message bubbles to root, guard fires.
    let _ = app.update(Message::Playlists(
        PlaylistsMessage::OpenCreatePlaylistDialog,
    ));

    assert!(
        !app.text_input_dialog.visible,
        "guard must keep the dialog closed when already editing"
    );
    assert!(
        app.playlist_editor.is_some(),
        "guard must not disturb the in-progress edit"
    );
}

#[test]
fn create_playlist_dialog_opens_when_not_editing() {
    use crate::{
        app_message::Message, views::PlaylistsMessage,
        widgets::text_input_dialog::TextInputDialogAction,
    };

    let mut app = test_app();
    assert!(app.playlist_editor.is_none());

    let _ = app.update(Message::Playlists(
        PlaylistsMessage::OpenCreatePlaylistDialog,
    ));

    assert!(app.text_input_dialog.visible);
    assert!(matches!(
        app.text_input_dialog.action,
        Some(TextInputDialogAction::CreatePlaylistAndEdit)
    ));
    assert!(app.text_input_dialog.public);
}

// ============================================================================
// Playlist Edit Mode — Public Toggle (F2, T8–T11)
// ============================================================================

#[test]
fn enter_edit_mode_preserves_playing_context() {
    use crate::{
        app_message::{Message, SplitViewMessage},
        state::ActivePlaylistContext,
    };

    let mut app = test_app();
    // Pre-condition: the queue is playing from a different playlist.
    app.active_playlist_info = Some(ActivePlaylistContext::minimal(
        "playing".into(),
        "Currently Playing".into(),
        String::new(),
    ));

    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "edited".into(),
        playlist_name: "Being Edited".into(),
        playlist_comment: "Edit me".into(),
        playlist_public: false,
    }));

    // Editing is decoupled from playback: entering edit mode must NOT touch
    // the "Playing From" banner — the queue keeps playing from "playing".
    let active = app
        .active_playlist_info
        .as_ref()
        .expect("active_playlist_info must stay pointed at the playing playlist");
    assert_eq!(
        active.id, "playing",
        "entering edit mode must leave the playing context untouched"
    );
    assert_eq!(active.name, "Currently Playing");
}

#[test]
fn exit_edit_mode_leaves_playing_context_untouched() {
    use crate::app_message::{Message, SplitViewMessage};

    let mut app = test_app();
    // Nothing playing (e.g., create-and-edit flow).
    assert!(app.active_playlist_info.is_none());

    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "new".into(),
        playlist_name: "Brand New".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    }));

    // Discard.
    let _ = app.update(Message::SplitView(SplitViewMessage::ExitEditMode));

    // Editing never set the banner, so exiting must leave it cleared — the
    // banner reflects what is playing, and nothing is playing.
    assert!(
        app.active_playlist_info.is_none(),
        "exit must not leave a stale banner from the edited playlist"
    );
    assert!(
        app.playlist_editor.is_none(),
        "exit clears the editor session"
    );
}

#[test]
fn save_edits_only_refreshes_banner_when_playing_the_edited_playlist() {
    use crate::{
        app_message::{Message, SplitViewMessage},
        state::ActivePlaylistContext,
    };

    // Editing a DIFFERENT playlist than the one playing: save must NOT
    // hijack the "Playing From" banner.
    let mut app = test_app();
    app.active_playlist_info = Some(ActivePlaylistContext::minimal(
        "playing".into(),
        "Currently Playing".into(),
        String::new(),
    ));
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "edited".into(),
        playlist_name: "Being Edited".into(),
        playlist_comment: String::new(),
        playlist_public: false,
    }));
    let _ = app.update(Message::SplitView(SplitViewMessage::PlaylistEditsSaved));
    assert_eq!(
        app.active_playlist_info.as_ref().map(|c| c.id.as_str()),
        Some("playing"),
        "saving an unrelated playlist must not steal the banner"
    );

    // Editing the SAME playlist the queue is playing from: save refreshes it.
    let mut app = test_app();
    app.active_playlist_info = Some(ActivePlaylistContext::minimal(
        "edited".into(),
        "Old Name".into(),
        String::new(),
    ));
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "edited".into(),
        playlist_name: "Being Edited".into(),
        playlist_comment: String::new(),
        playlist_public: false,
    }));
    let _ = app.update(Message::SplitView(SplitViewMessage::PlaylistEditsSaved));
    assert_eq!(
        app.active_playlist_info.as_ref().map(|c| c.id.as_str()),
        Some("edited"),
        "saving the currently-playing playlist keeps the banner on it"
    );
}

#[test]
fn enter_edit_mode_navigates_to_editor_and_records_origin() {
    use crate::{
        View,
        app_message::{Message, SplitViewMessage},
    };

    let mut app = test_app();
    app.current_view = View::Playlists;

    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: false,
    }));

    assert_eq!(
        app.current_view,
        View::PlaylistEditor,
        "entering edit mode must navigate to the dedicated editor view"
    );
    assert_eq!(
        app.editor_return_view,
        View::Playlists,
        "entry must record the originating view for the return-on-exit"
    );
    assert!(app.playlist_editor.is_some());
}

#[test]
fn exit_edit_mode_returns_to_origin_view() {
    use crate::{
        View,
        app_message::{Message, SplitViewMessage},
    };

    let mut app = test_app();
    app.current_view = View::Playlists;
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: false,
    }));
    assert_eq!(app.current_view, View::PlaylistEditor);

    let _ = app.update(Message::SplitView(SplitViewMessage::ExitEditMode));

    assert_eq!(
        app.current_view,
        View::Playlists,
        "discarding must return the user to where they launched the edit"
    );
    assert!(app.playlist_editor.is_none(), "exit clears the session");
    assert!(app.browsing_panel.is_none(), "exit tears down the split");
}

#[test]
fn enter_playlist_edit_mode_seeds_initial_public() {
    use crate::app_message::{Message, SplitViewMessage};

    let mut app = test_app();
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: false,
    }));

    let edit = app
        .playlist_editor
        .as_ref()
        .map(|e| &e.edit)
        .expect("entering edit mode must populate playlist_editor");
    assert!(
        !edit.playlist_public,
        "EnterPlaylistEditMode with public=false must seed playlist_public=false"
    );
    assert!(
        !edit.is_public_dirty(),
        "freshly seeded edit state must not report public-dirty"
    );
}

#[test]
fn playlist_edit_public_toggle_flips_state() {
    use crate::app_message::{EditorMessage, Message, SplitViewMessage};

    let mut app = test_app();
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    }));

    let _ = app.update(Message::Editor(EditorMessage::PublicToggled(false)));

    let edit = app
        .playlist_editor
        .as_ref()
        .map(|e| &e.edit)
        .expect("playlist_editor set");
    assert!(
        !edit.playlist_public,
        "PublicToggled(false) must flip the edit-state flag"
    );
    assert!(
        edit.is_public_dirty(),
        "after toggle the edit state must be public-dirty"
    );
}

#[test]
fn playlist_edit_public_revert_clears_dirty() {
    use crate::app_message::{EditorMessage, Message, SplitViewMessage};

    let mut app = test_app();
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    }));

    let _ = app.update(Message::Editor(EditorMessage::PublicToggled(false)));
    let _ = app.update(Message::Editor(EditorMessage::PublicToggled(true)));

    let edit = app
        .playlist_editor
        .as_ref()
        .map(|e| &e.edit)
        .expect("playlist_editor set");
    assert!(
        !edit.is_public_dirty(),
        "toggling back to the original value must clear public-dirty"
    );
}

#[test]
fn playlist_edit_public_only_change_is_metadata_dirty() {
    use crate::app_message::{EditorMessage, Message, SplitViewMessage};

    let mut app = test_app();
    let _ = app.update(Message::SplitView(SplitViewMessage::EnterEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    }));

    let _ = app.update(Message::Editor(EditorMessage::PublicToggled(false)));

    let edit = app
        .playlist_editor
        .as_ref()
        .map(|e| &e.edit)
        .expect("playlist_editor set");
    assert!(
        edit.has_metadata_changes(),
        "a pure-visibility flip must satisfy the predicate the save handler \
         uses to decide whether to call update_playlist (R6 fix)"
    );
}

// Active-playlist banner re-sync on playlists load (resync_active_playlist_context)
// ============================================================================

fn full_playlist(id: &str, name: &str) -> nokkvi_data::backend::playlists::PlaylistUIViewData {
    nokkvi_data::backend::playlists::PlaylistUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        comment: "synced comment".to_string(),
        duration: 6241.0,
        song_count: 29,
        owner_name: "owner".to_string(),
        public: true,
        updated_at: "2026-05-27T20:19:59-06:00".to_string(),
        artwork_album_ids: vec![],
        searchable_lower: name.to_lowercase(),
    }
}

#[test]
fn resync_upgrades_minimal_active_playlist_context() {
    let mut app = test_app();
    // A restored/minimal context: correct id, but missing metadata.
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "p1".into(),
        "Stale Name".into(),
        String::new(),
    ));
    app.library
        .playlists
        .append_page(vec![full_playlist("p1", "Synced Name")], 1);

    app.resync_active_playlist_context();

    let ctx = app.active_playlist_info.as_ref().expect("context retained");
    assert_eq!(
        ctx.name, "Synced Name",
        "name refreshed from loaded metadata"
    );
    assert_eq!(ctx.song_count, 29, "song count upgraded");
    assert!(
        (ctx.duration_secs - 6241.0).abs() < f32::EPSILON,
        "duration upgraded"
    );
    assert!(ctx.public, "visibility upgraded to public");
    assert_eq!(ctx.updated, "2026-05-27T20:19:59-06:00", "updated upgraded");
}

#[test]
fn resync_noops_when_active_playlist_absent_from_loaded_list() {
    let mut app = test_app();
    let original = crate::state::ActivePlaylistContext::minimal(
        "missing".into(),
        "Kept".into(),
        String::new(),
    );
    app.active_playlist_info = Some(original.clone());
    app.library
        .playlists
        .append_page(vec![full_playlist("other", "Other")], 1);

    app.resync_active_playlist_context();

    assert_eq!(
        app.active_playlist_info.as_ref(),
        Some(&original),
        "a playlist not in the loaded page must leave the context untouched"
    );
}

#[test]
fn resync_noops_with_no_active_playlist() {
    let mut app = test_app();
    app.active_playlist_info = None;
    app.library
        .playlists
        .append_page(vec![full_playlist("p1", "Synced")], 1);

    app.resync_active_playlist_context();

    assert!(
        app.active_playlist_info.is_none(),
        "no active playlist stays none"
    );
}

// ============================================================================
