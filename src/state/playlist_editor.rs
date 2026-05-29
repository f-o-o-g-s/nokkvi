//! Playlist editor state — the editor's OWN in-memory working buffer.
//!
//! Holds the playlist-being-edited's tracks in a buffer that is fully
//! decoupled from the live play queue. The presence of `Some(..)` on
//! `Nokkvi.playlist_editor` is the "in edit mode" signal.
//!
//! Entering edit mode populates `songs` via an async resolve, navigates to
//! `View::PlaylistEditor`, and routes all mutations and Save through this
//! buffer — the live play queue is never read or written during a session.

use nokkvi_data::{backend::queue::QueueSongUIViewData, types::playlist_edit::PlaylistEditState};

use crate::{views::queue::QueueColumnVisibility, widgets::SlotListPageState};

/// State for an active playlist editing session that owns its own track
/// buffer, leaving the live play queue untouched.
///
/// Reuses the queue's row type (`QueueSongUIViewData`) and slot-list state
/// (`SlotListPageState`) so the shared `song_list_pane` renderer can draw
/// the editor identically to the queue, while keeping a separate, local
/// `Vec` working surface with no backend round-trip per edit.
#[derive(Debug)]
pub struct PlaylistEditorState {
    /// The editor's own track buffer (reuses the queue row type).
    pub songs: Vec<QueueSongUIViewData>,
    /// Shared slot-list state: search, scroll, focus, multi-selection.
    /// Independent of the queue page's slot list — no shared cursor.
    pub common: SlotListPageState,
    /// Dirty-detection metadata (name/comment/public + saved snapshot).
    pub edit: PlaylistEditState,
    /// Per-column visibility flags, mirroring the queue page's columns.
    pub columns: QueueColumnVisibility,
}

impl PlaylistEditorState {
    /// Create an editor session from its dirty-detection metadata.
    ///
    /// `songs` starts empty (filled when the async resolve returns via
    /// `EditorMessage::SongsLoaded`), `common` uses the queue's sort-less
    /// slot-list shape, and `columns` defaults to a fresh queue page's.
    pub fn new(edit: PlaylistEditState) -> Self {
        Self {
            songs: Vec::new(),
            common: SlotListPageState::new_without_sort_mode(),
            edit,
            columns: QueueColumnVisibility::default(),
        }
    }
}
