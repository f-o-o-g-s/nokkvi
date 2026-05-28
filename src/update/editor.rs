//! Playlist-editor message handler.
//!
//! The editor operates on its OWN in-memory track buffer
//! (`Nokkvi.playlist_editor`), fully decoupled from the live play queue.
//!
//! Phase 3a wires the async resolve result (`EditorMessage::SongsLoaded`) into
//! the buffer and seeds the dirty snapshot so a freshly-loaded session is
//! clean. Remaining variants (reorder/remove/add, metadata edits, Save) stay
//! no-ops until later phases.

use iced::Task;
use nokkvi_data::backend::queue::QueueSongUIViewData;

use crate::{
    Nokkvi,
    app_message::{EditorMessage, Message},
};

impl Nokkvi {
    /// Dispatch a [`EditorMessage`].
    //
    // Phase 4+ fills in the buffer-mutation / metadata / save variants.
    pub(crate) fn handle_editor_message(&mut self, msg: EditorMessage) -> Task<Message> {
        match msg {
            EditorMessage::SongsLoaded(rows) => self.handle_editor_songs_loaded(rows),
            EditorMessage::SlotList(m) => self.handle_editor_slot_list(m),
            EditorMessage::NameChanged(name) => {
                if let Some(editor) = self.playlist_editor.as_mut() {
                    editor.edit.set_name(name);
                }
                Task::none()
            }
            EditorMessage::CommentChanged(comment) => {
                if let Some(editor) = self.playlist_editor.as_mut() {
                    editor.edit.set_comment(comment);
                }
                Task::none()
            }
            EditorMessage::PublicToggled(value) => {
                if let Some(editor) = self.playlist_editor.as_mut() {
                    editor.edit.set_public(value);
                }
                Task::none()
            }
            // Discard/exit reuses the shared split-view exit handler — the
            // editor view emits this so the discard button can route through
            // the editor's own message space (Phase 6 owns the exit handler).
            EditorMessage::ExitEditMode => Task::done(Message::SplitView(
                crate::app_message::SplitViewMessage::ExitEditMode,
            )),
            // Per-row context-menu open/close — forward to the single overlay
            // stack so editor menus share the same close-on-outside-click path.
            EditorMessage::SetOpenMenu(menu) => self.handle_set_open_menu(menu),
            // Buffer mutations and Save land in later phases.
            EditorMessage::DragReorder(_)
            | EditorMessage::RemoveAt(_)
            | EditorMessage::ContextMenuAction(..)
            | EditorMessage::Save => Task::none(),
        }
    }

    /// Apply a shared slot-list message to the editor's OWN slot-list state.
    ///
    /// Mirrors how the queue page routes `SlotListPageMessage` through
    /// `SlotListPageState::handle`, but against `playlist_editor.common` — the
    /// editor keeps an independent cursor/selection from the live queue. The
    /// total item count is the editor buffer's current length so navigation and
    /// selection clamp correctly.
    fn handle_editor_slot_list(
        &mut self,
        msg: crate::widgets::SlotListPageMessage,
    ) -> Task<Message> {
        if let Some(editor) = self.playlist_editor.as_mut() {
            let total = editor.songs.len();
            // The editor has no sort/play side effects to act on — the returned
            // action is intentionally discarded (search/sort/activate variants
            // are not surfaced by the editor's row vocabulary).
            let _ = editor.common.handle(msg, total);
        }
        Task::none()
    }

    /// Fill the editor buffer with the async-resolved playlist rows.
    ///
    /// Seeds the dirty snapshot from the loaded rows so a freshly-loaded
    /// session is clean (fixes bug 10: `PlaylistEditState::new` seeds an empty
    /// snapshot, leaving the session always-dirty until re-seeded), and clears
    /// any stale slot-list selection (mirrors `handle_queue_loaded`).
    pub(crate) fn handle_editor_songs_loaded(
        &mut self,
        rows: Vec<QueueSongUIViewData>,
    ) -> Task<Message> {
        if let Some(editor) = self.playlist_editor.as_mut() {
            editor.songs = rows;
            // Seed the dirty snapshot from the rows just stored so the session
            // starts pristine.
            let loaded_ids: Vec<String> = editor.songs.iter().map(|s| s.id.clone()).collect();
            editor.edit.update_snapshot(loaded_ids);
            // Drop any stale multi-selection — the loaded rows may not line up
            // with whatever was selected in a prior session.
            editor.common.slot_list.selected_indices.clear();
            editor.common.slot_list.anchor_index = None;
        }
        Task::none()
    }
}
