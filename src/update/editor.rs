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
            // Buffer mutations, metadata edits, and Save land in later phases.
            EditorMessage::SlotList(_)
            | EditorMessage::DragReorder(_)
            | EditorMessage::RemoveAt(_)
            | EditorMessage::ContextMenuAction(..)
            | EditorMessage::NameChanged(_)
            | EditorMessage::CommentChanged(_)
            | EditorMessage::PublicToggled(_)
            | EditorMessage::Save => Task::none(),
        }
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
