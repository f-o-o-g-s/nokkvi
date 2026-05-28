//! Playlist editor view.
//!
//! The editor renders the playlist-being-edited's OWN track buffer (decoupled
//! from the live play queue). It shares the queue's row/column/drag/search
//! rendering via the `song_list_pane` builder (Phase 2) but has no "now
//! playing" concept — it drops every playback field that `QueueViewData`
//! carries.
//!
//! Phase 1 only lands [`EditorViewData`] so later phases compile. The editor's
//! `view()` rendering is built in Phase 3.

use std::borrow::Cow;

use nokkvi_data::backend::queue::QueueSongUIViewData;

use crate::views::queue::QueueColumnVisibility;

/// Read-only view data passed from root to the playlist editor.
///
/// A trimmed [`crate::views::QueueViewData`] that **borrows** the editor's
/// track buffer. Drops all playback fields (`current_playing_song_id`,
/// `current_playing_entry_id`, `is_playing`, playlist context/cover) — the
/// editor never reflects a "now playing" row.
//
// Phase 3 wires this struct into the editor's `view()` (which will widen
// visibility to `pub`, matching `QueueViewData`); until then every field is
// dormant.
#[allow(dead_code)]
pub(crate) struct EditorViewData<'a> {
    /// The editor's track buffer, borrowed (filtered when a search is active).
    pub songs: Cow<'a, [QueueSongUIViewData]>,
    /// Album-art thumbnail cache (80px), keyed by album_id.
    pub album_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    /// Large-artwork cache for the artwork column fallback.
    pub large_artwork: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    /// Total buffer count before filtering (for empty-state detection).
    pub total_count: usize,
    /// Per-column visibility flags (mirrors the queue page).
    pub columns: QueueColumnVisibility,
    /// Edit-bar: the playlist's current (editable) name.
    pub name: String,
    /// Edit-bar: the playlist's current (editable) comment.
    pub comment: String,
    /// Edit-bar: the playlist's current public flag (drives the lock toggle).
    pub public: bool,
    /// Edit-bar: whether the editor has unsaved changes (tracks or metadata).
    pub dirty: bool,
    /// Visual slot index where the cross-pane-drag drop indicator should draw —
    /// `Some` only when a drag is active and the cursor is over an editor slot.
    pub drop_indicator_slot: Option<usize>,
}
