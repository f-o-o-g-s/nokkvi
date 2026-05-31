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

/// Async-load lifecycle of an editor session's track buffer.
///
/// Entering edit mode constructs the editor with an EMPTY buffer and navigates
/// to the editor view BEFORE the async resolve returns, so an empty buffer is
/// otherwise indistinguishable from "loading", "failed", or "genuinely empty".
/// This marker disambiguates them: save and track mutations are gated on
/// `Loaded`, so a failed/in-flight resolve can never full-overwrite the real
/// server playlist with a partial (or empty) buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorLoadState {
    /// Resolve in flight — buffer not yet populated.
    Loading,
    /// Resolve succeeded — the buffer reflects the server playlist.
    Loaded,
    /// Resolve failed — the buffer is unreliable; save/mutations are blocked.
    Failed,
}

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
    /// Async-resolve lifecycle of the track buffer. Save and track mutations
    /// are gated on `Loaded` so a failed/in-flight resolve can never overwrite
    /// the server playlist with a partial buffer.
    pub load_state: EditorLoadState,
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
            // Starts Loading: the buffer fills once the async resolve returns
            // via `EditorMessage::SongsLoaded` (→ Loaded) or `SongsLoadFailed`
            // (→ Failed). Defaulting inside `new()` keeps every call site (one
            // production, several tests) unchanged.
            load_state: EditorLoadState::Loading,
        }
    }
}
