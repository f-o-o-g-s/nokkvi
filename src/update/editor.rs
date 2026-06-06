//! Playlist-editor message handler.
//!
//! The editor operates on its OWN in-memory track buffer
//! (`Nokkvi.playlist_editor`), fully decoupled from the live play queue.
//! `EditorMessage::SongsLoaded` fills the buffer from the async resolve and
//! seeds the dirty snapshot; reorder/remove/add mutate the buffer in place;
//! metadata edits update the edit state; Save persists the buffer and exit
//! tears the session down — none of which touch the live queue.

use std::collections::HashSet;

use iced::Task;
use nokkvi_data::backend::queue::QueueSongUIViewData;

use super::components::{passive_artwork_version, prefetch_album_artwork_tasks};
use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, EditorMessage, Message},
    views::queue::QueueContextEntry,
    widgets::drag_column::DragEvent,
};

impl Nokkvi {
    /// Dispatch a [`EditorMessage`].
    pub(crate) fn handle_editor_message(&mut self, msg: EditorMessage) -> Task<Message> {
        match msg {
            EditorMessage::SongsLoaded(rows) => self.handle_editor_songs_loaded(rows),
            EditorMessage::SongsLoadFailed => {
                // Mark the session Failed so save + track mutations are gated
                // off — the empty buffer is NOT the real playlist. The editor
                // stays mounted (no auto-abort); Discard/Exit remain available.
                // (The detailed error was already logged at the resolve site.)
                if let Some(editor) = self.playlist_editor.as_mut() {
                    editor.load_state = crate::state::EditorLoadState::Failed;
                }
                self.toast_error("Failed to load playlist for editing");
                Task::none()
            }
            EditorMessage::SongsInserted { rows, at } => {
                self.handle_editor_songs_inserted(rows, at)
            }
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
            // Buffer mutations: plain in-memory `Vec` ops on the editor buffer,
            // no queue/engine/redb round-trip. Dirty detection is computed at
            // render time from the buffer, so these need no explicit dirty flag.
            EditorMessage::DragReorder(event) => self.handle_editor_drag_reorder(event),
            EditorMessage::RemoveAt(idx) => self.handle_editor_remove_at(idx),
            EditorMessage::ContextMenuAction(idx, entry) => {
                self.handle_editor_context_menu_action(idx, entry)
            }
            // Forward to the shared save handler (same pattern as ExitEditMode)
            // so the editor reuses the single `handle_save_playlist_edits` path
            // rather than duplicating the playlists-API logic.
            EditorMessage::Save => Task::done(Message::SplitView(
                crate::app_message::SplitViewMessage::SavePlaylistEdits,
            )),
        }
    }

    /// Reorder the editor buffer in response to a drag-and-drop.
    ///
    /// Mirrors the queue's `DragReorder` handler (`views/queue/update.rs:117`):
    /// single-row drag is **guarded while a search query is active** (mirror of
    /// the queue's `:119` guard) because filtered slot indices would otherwise
    /// move the wrong row (invariant #1). With no search, slot indices map to
    /// buffer indices through the editor's own slot-list, then a plain
    /// `remove`+`insert` reorders the buffer. Unlike the queue, there is no
    /// backend `MoveItem`/`MoveBatch` round-trip — the buffer is the source of
    /// truth until Save (Phase 6).
    /// Whether the editor buffer reflects the server playlist and may be
    /// mutated/saved. A still-loading or failed resolve leaves an empty/partial
    /// buffer, so track mutations are blocked until `Loaded` (the save gate in
    /// `handle_save_playlist_edits` is the matching guard).
    fn editor_is_loaded(&self) -> bool {
        self.playlist_editor
            .as_ref()
            .is_some_and(|e| e.load_state == crate::state::EditorLoadState::Loaded)
    }

    fn handle_editor_drag_reorder(&mut self, event: DragEvent) -> Task<Message> {
        // Block buffer mutations until the resolve has loaded the real tracks.
        if !self.editor_is_loaded() {
            return Task::none();
        }
        let Some(editor) = self.playlist_editor.as_mut() else {
            return Task::none();
        };
        // Guard while a search query is active — same as the queue. A filtered
        // view changes the slot→item mapping, so a raw reorder is unsafe.
        if !editor.common.search_query.is_empty() {
            return Task::none();
        }

        let total = editor.songs.len();
        match event {
            DragEvent::Picked { index } => {
                // Match the queue: highlight the picked row unless it's part of
                // an existing multi-selection (batch drag preserves it).
                if let Some(item_index) = editor.common.slot_list.slot_to_item_index(index, total)
                    && !editor
                        .common
                        .slot_list
                        .selected_indices
                        .contains(&item_index)
                {
                    editor.common.slot_list.set_selected(item_index, total);
                }
                Task::none()
            }
            DragEvent::Dropped {
                index,
                target_index,
            } => {
                let from = editor.common.slot_list.slot_to_item_index(index, total);
                let to = editor
                    .common
                    .slot_list
                    .slot_to_item_index_for_drop(target_index, total);

                // Multi-selection batch drag: if several rows are selected and
                // the dragged row is one of them, move the whole batch — same
                // condition the queue uses before dispatching `MoveBatch`.
                let selected = &editor.common.slot_list.selected_indices;
                if selected.len() > 1
                    && from.is_some_and(|f| selected.contains(&f))
                    && let Some(t) = to
                {
                    let mut indices: Vec<usize> = selected.iter().copied().collect();
                    editor.common.clear_multi_selection();
                    Self::reorder_buffer_batch(&mut editor.songs, &mut indices, t);
                } else if let (Some(f), Some(t)) = (from, to)
                    && f != t
                {
                    let item = editor.songs.remove(f);
                    let insert_at = if f < t { t - 1 } else { t };
                    editor.songs.insert(insert_at, item);
                    // Keep the highlight on the moved row at its new position
                    // (mirrors the queue's `set_selected(insert_at, ..)`).
                    let new_total = editor.songs.len();
                    editor.common.slot_list.set_selected(insert_at, new_total);
                }
                Task::none()
            }
            // A cancelled drag leaves the buffer untouched (matches the queue's
            // catch-all no-op for non-drop events).
            DragEvent::Canceled { .. } => Task::none(),
        }
    }

    /// In-memory batch reorder mirroring the queue's `MoveBatch` optimistic
    /// local reorder (`update/queue.rs:296`): remove the selected rows
    /// (descending so earlier removals don't shift later indices), then insert
    /// them as a contiguous block before `target`, adjusting the insert point
    /// for rows removed from before the target.
    fn reorder_buffer_batch(
        songs: &mut Vec<QueueSongUIViewData>,
        indices: &mut [usize],
        target: usize,
    ) {
        indices.sort_unstable_by(|a, b| b.cmp(a)); // descending
        let len = songs.len();
        let mut moved = Vec::new();
        for &i in indices.iter() {
            if i < songs.len() {
                moved.push(songs.remove(i));
            }
        }
        moved.reverse(); // restore ascending order for insertion

        let removed_before_target = indices.iter().filter(|&&i| i < target).count();
        let adjusted_target = target.min(len).saturating_sub(removed_before_target);
        let insert_pos = adjusted_target.min(songs.len());
        for (offset, song) in moved.into_iter().enumerate() {
            songs.insert(insert_pos + offset, song);
        }
    }

    /// Remove from the editor buffer at a slot index (multi-selection aware).
    ///
    /// Mirrors the queue's context-menu remove (`views/queue/update.rs:198` +
    /// `update/queue.rs:377`): `evaluate_context_menu` expands the clicked row
    /// to the full multi-selection when the clicked row is selected, otherwise
    /// targets just that row. Targets are resolved to per-row `entry_id`s at the
    /// boundary so removal is duplicate-aware (two rows of the same song_id only
    /// lose the targeted one) and drift-immune. Filtered indices are mapped
    /// through the filtered view first (invariant #1).
    fn handle_editor_remove_at(&mut self, idx: usize) -> Task<Message> {
        // Block buffer mutations until the resolve has loaded the real tracks.
        if !self.editor_is_loaded() {
            return Task::none();
        }
        // Resolve the (possibly filtered) slot index to per-row entry_id(s)
        // BEFORE the mutable borrow — under an active search the index is
        // relative to the filtered view, so map through it (invariant #1).
        // `entry_id` lookup makes removal duplicate-aware and drift-immune.
        let target_entry_ids: Vec<u64> = {
            let filtered = self.filter_editor_songs();
            let Some(editor) = self.playlist_editor.as_ref() else {
                return Task::none();
            };
            // Expand the clicked index to the multi-selection when the clicked
            // row is selected, else just that row — same as the queue's
            // `evaluate_context_menu`. Read-only resolution here; the selection
            // mutation happens below under the mutable borrow.
            let target_indices: Vec<usize> =
                if editor.common.slot_list.selected_indices.contains(&idx) {
                    editor
                        .common
                        .slot_list
                        .selected_indices
                        .iter()
                        .copied()
                        .collect()
                } else {
                    vec![idx]
                };
            target_indices
                .iter()
                .filter_map(|&i| filtered.get(i).map(|s| s.entry_id))
                .collect()
        };

        let Some(editor) = self.playlist_editor.as_mut() else {
            return Task::none();
        };
        editor.common.clear_multi_selection();
        if target_entry_ids.is_empty() {
            return Task::none();
        }

        let id_set: std::collections::HashSet<u64> = target_entry_ids.into_iter().collect();
        editor.songs.retain(|s| !id_set.contains(&s.entry_id));

        // Clean up the slot-list cursor/selection so nothing dangles past the
        // shrunk buffer (mirrors `handle_queue_loaded`'s cleanup at
        // `update/queue.rs:46`/`:85`): drop the click-to-focus marker and clamp
        // the viewport offset into range. `evaluate_context_menu` +
        // `clear_multi_selection` above already cleared the multi-selection.
        let new_total = editor.songs.len();
        editor.common.slot_list.clear_focus_cursor();
        if new_total > 0 && editor.common.slot_list.viewport_offset >= new_total {
            editor.common.slot_list.viewport_offset = new_total.saturating_sub(1);
        } else if new_total == 0 {
            editor.common.slot_list.viewport_offset = 0;
        }
        Task::none()
    }

    /// Handle an editor row's context-menu entry.
    ///
    /// "Remove from Playlist" routes through the buffer remove path;
    /// "Get Info" stays a no-op for now (the editor surfaces removal + metadata
    /// edits, not the queue's full navigation menu). The remaining queue menu
    /// entries are not offered by the editor view and fall through to no-op.
    fn handle_editor_context_menu_action(
        &mut self,
        idx: usize,
        entry: QueueContextEntry,
    ) -> Task<Message> {
        match entry {
            QueueContextEntry::RemoveFromQueue => self.handle_editor_remove_at(idx),
            // Get Info / everything else: no-op in the editor for now.
            _ => Task::none(),
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
        // Prefetch mini artwork for rows scrolled into view (mirrors the
        // queue's slot-list-change prefetch).
        self.editor_artwork_prefetch_tasks()
    }

    /// Dispatch mini-artwork prefetch for the editor buffer's visible rows.
    ///
    /// The editor reads thumbnails from the shared `artwork.album_art` cache
    /// (the same snapshot the queue and library panes pass), so without this
    /// the rows render blank gray placeholders for any album not already
    /// fetched by another view. Mirrors `handle_queue_loaded`: the canonical
    /// 80 px album-id prefetch plus a large-artwork load for the centered row.
    /// A no-op when there is no backend or the buffer is empty.
    fn editor_artwork_prefetch_tasks(&self) -> Task<Message> {
        let (Some(editor), Some(shell)) =
            (self.playlist_editor.as_ref(), self.app_service.as_ref())
        else {
            return Task::none();
        };
        if editor.songs.is_empty() {
            return Task::none();
        }

        let cached: HashSet<&String> = self.artwork.album_art.iter().map(|(k, _)| k).collect();
        let mut tasks = prefetch_album_artwork_tasks(
            &editor.common.slot_list,
            &editor.songs,
            &cached,
            &self.artwork.album_art_versions,
            shell.albums().clone(),
            |song| {
                (
                    song.album_id.clone(),
                    passive_artwork_version(&song.updated_at),
                    song.artwork_url.clone(),
                )
            },
        );

        // Large artwork for the centered row, so the artwork panel fills too.
        if let Some(center_idx) = editor
            .common
            .slot_list
            .get_center_item_index(editor.songs.len())
            && let Some(song) = editor.songs.get(center_idx)
        {
            tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                song.album_id.clone(),
            ))));
        }

        Task::batch(tasks)
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
            // The resolve succeeded — mark Loaded so save + track mutations are
            // allowed (the buffer now reflects the server playlist). The
            // create-and-edit-empty flow resolves to an empty Ok(vec) → Loaded,
            // so saving an intentionally-empty playlist is correctly allowed.
            editor.load_state = crate::state::EditorLoadState::Loaded;
            // Seed the dirty snapshot from the rows just stored so the session
            // starts pristine.
            let loaded_ids: Vec<String> = editor.songs.iter().map(|s| s.id.clone()).collect();
            editor.edit.update_snapshot(loaded_ids);
            // Drop any stale multi-selection — the loaded rows may not line up
            // with whatever was selected in a prior session.
            editor.common.slot_list.clear_multi_selection();
        }
        // Prefetch mini artwork for the freshly-loaded buffer so the rows show
        // their covers instead of blank gray placeholders. The editor never
        // otherwise populates the shared `artwork.album_art` cache for its own
        // songs (mirrors `handle_queue_loaded`).
        self.editor_artwork_prefetch_tasks()
    }

    /// Splice cross-pane-dragged rows into the editor buffer at a drop slot.
    ///
    /// The async resolve result of a browser→editor drop
    /// (`EditorMessage::SongsInserted`). The rows arrive with placeholder
    /// `entry_id`s from the resolve projection, so this assigns FRESH
    /// sequential ids (`max(existing) + 1 ..`) that cannot collide with any
    /// existing buffer row — the editor addresses rows by `entry_id` for
    /// duplicate-aware removal (Phase 4), so collisions would let one remove
    /// take out an unrelated row.
    ///
    /// `at` is the drop slot relative to the editor's current (possibly
    /// filtered) view, so it is mapped through `filter_editor_songs()` to a
    /// full-buffer insert position before splicing (invariant #1). With no
    /// search active this is the identity case. The position is clamped to the
    /// buffer length so a stale / out-of-range slot appends rather than
    /// panicking.
    fn handle_editor_songs_inserted(
        &mut self,
        rows: Vec<QueueSongUIViewData>,
        at: usize,
    ) -> Task<Message> {
        if rows.is_empty() {
            return Task::none();
        }
        // Block buffer mutations until the resolve has loaded the real tracks —
        // a cross-pane drop into a still-loading/failed session must not splice
        // into an empty/partial buffer that would then full-overwrite on save.
        if !self.editor_is_loaded() {
            return Task::none();
        }

        // Map the (possibly filtered) drop slot to a full-buffer index BEFORE
        // taking the mutable borrow. Under an active search the slot is
        // relative to the filtered view: insert AFTER the row at the matching
        // full-buffer position (or at the end when dropping past the filtered
        // tail), so the dragged rows land where the indicator showed them.
        let insert_at = {
            let filtered = self.filter_editor_songs();
            let Some(editor) = self.playlist_editor.as_ref() else {
                return Task::none();
            };
            if editor.common.search_query.is_empty() {
                at.min(editor.songs.len())
            } else {
                match filtered.get(at) {
                    // Insert before the matching full-buffer row.
                    Some(target) => editor
                        .songs
                        .iter()
                        .position(|s| s.entry_id == target.entry_id)
                        .unwrap_or(editor.songs.len()),
                    // Dropped past the filtered tail → append.
                    None => editor.songs.len(),
                }
            }
        };

        let Some(editor) = self.playlist_editor.as_mut() else {
            return Task::none();
        };

        // Fresh sequential entry_ids starting past the current max so the new
        // rows never collide with existing buffer ids.
        let base_id = editor
            .songs
            .iter()
            .map(|s| s.entry_id)
            .max()
            .map_or(0, |m| m + 1);
        let insert_at = insert_at.min(editor.songs.len());
        for (offset, mut row) in rows.into_iter().enumerate() {
            row.entry_id = base_id + offset as u64;
            editor.songs.insert(insert_at + offset, row);
        }

        // Drop any stale selection so it does not point at shifted rows.
        editor.common.clear_selection_for_refresh();
        // Fetch art for the newly dropped rows.
        self.editor_artwork_prefetch_tasks()
    }
}
