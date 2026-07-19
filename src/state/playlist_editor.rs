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

/// What an editor session edits. Under the workspace
/// `match_wildcard_for_single_variants = deny` lint, every match on the
/// session kind is a compile-visible decision point — the intended safety
/// property for keeping the two modes from drifting into each other.
#[derive(Debug)]
pub enum EditorSessionKind {
    /// The classic track-buffer editor (existing behavior).
    Tracks,
    /// The smart-playlist rules editor: right pane = rule form, left pane
    /// = evaluated tracks. Track mutations are structurally absent — the
    /// server refuses them on smart playlists anyway.
    Rules(Box<crate::state::RulesSessionUi>),
}

impl EditorSessionKind {
    /// The rules session, when this is a Rules editor.
    pub fn rules(&self) -> Option<&crate::state::RulesSessionUi> {
        match self {
            EditorSessionKind::Rules(session) => Some(session),
            EditorSessionKind::Tracks => None,
        }
    }

    /// Mutable rules access.
    pub fn rules_mut(&mut self) -> Option<&mut crate::state::RulesSessionUi> {
        match self {
            EditorSessionKind::Rules(session) => Some(session),
            EditorSessionKind::Tracks => None,
        }
    }
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
    /// Tracks (existing behavior) vs Rules (the smart-playlist editor).
    pub kind: EditorSessionKind,
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
    /// A deferred-create Save is in flight (the row is being minted). Blocks a
    /// second Save (double-click would mint duplicate playlists — the id isn't
    /// adopted until the async create returns) and blocks discard. Set true
    /// when the create task spawns; cleared on create success/failure.
    pub creating: bool,
    /// Per-row `entry_id`s grabbed by an in-progress within-list drag,
    /// snapshotted at pick time so the drop resolves its source by identity
    /// (immune to a mid-drag viewport shift) and the floating drag ghost can
    /// render the grabbed row. `None` when no drag is active. Mirrors
    /// `QueuePage.drag_source`.
    pub drag_source: Option<Vec<u64>>,
    /// Live RAW cursor of an in-progress within-list drag, driving the floating
    /// identity ghost. `None` between the pick and the first cursor move.
    pub drag_cursor: Option<iced::Point>,
    /// Which vertical edge band the drag cursor sits in — drives tick auto-scroll.
    pub drag_edge: crate::widgets::drag_column::EdgeZone,
    /// Live drop-target slot for the drag, feeding the drop-indicator line.
    pub drag_target_slot: Option<usize>,
}

impl PlaylistEditorState {
    /// Create an editor session from its dirty-detection metadata.
    ///
    /// `songs` starts empty (filled when the async resolve returns via
    /// `EditorMessage::SongsLoaded`), `common` uses the queue's sort-less
    /// slot-list shape, and `columns` defaults to a fresh queue page's.
    pub fn new(edit: PlaylistEditState) -> Self {
        Self {
            kind: EditorSessionKind::Tracks,
            songs: Vec::new(),
            common: SlotListPageState::new_without_sort_mode(),
            edit,
            columns: QueueColumnVisibility::default(),
            // Starts Loading: the buffer fills once the async resolve returns
            // via `EditorMessage::SongsLoaded` (→ Loaded) or `SongsLoadFailed`
            // (→ Failed). Defaulting inside `new()` keeps every call site (one
            // production, several tests) unchanged.
            load_state: EditorLoadState::Loading,
            creating: false,
            drag_source: None,
            drag_cursor: None,
            drag_edge: crate::widgets::drag_column::EdgeZone::None,
            drag_target_slot: None,
        }
    }

    /// Create a BLANK Tracks session for the drop-into-editor create flow —
    /// no server playlist exists yet (`edit.playlist_id` is empty). The buffer
    /// starts empty and `load_state` is `Loaded` (there is no track resolve to
    /// wait for); the row is minted on the first Save (deferred create),
    /// mirroring the smart-create flow.
    pub fn new_create(edit: PlaylistEditState) -> Self {
        Self {
            kind: EditorSessionKind::Tracks,
            songs: Vec::new(),
            common: SlotListPageState::new_without_sort_mode(),
            edit,
            columns: QueueColumnVisibility::default(),
            load_state: EditorLoadState::Loaded,
            creating: false,
            drag_source: None,
            drag_cursor: None,
            drag_edge: crate::widgets::drag_column::EdgeZone::None,
            drag_target_slot: None,
        }
    }

    /// Create a RULES editor session. `edit` carries the edit-bar metadata
    /// (name/comment/public + the dirty snapshot); the rules session holds
    /// the form. The track buffer stays empty and `load_state` starts
    /// `Loaded` — there is no track resolve to wait for, and the Loaded
    /// gate must not block the rules Save path.
    pub fn new_rules(edit: PlaylistEditState, session: crate::state::RulesSessionUi) -> Self {
        Self {
            kind: EditorSessionKind::Rules(Box::new(session)),
            songs: Vec::new(),
            common: SlotListPageState::new_without_sort_mode(),
            edit,
            columns: QueueColumnVisibility::default(),
            load_state: EditorLoadState::Loaded,
            creating: false,
            drag_source: None,
            drag_cursor: None,
            drag_edge: crate::widgets::drag_column::EdgeZone::None,
            drag_target_slot: None,
        }
    }

    /// The rules session, when this is a Rules editor.
    pub fn rules_session(&self) -> Option<&crate::state::RulesSessionUi> {
        self.kind.rules()
    }

    /// Mutable rules session access.
    pub fn rules_session_mut(&mut self) -> Option<&mut crate::state::RulesSessionUi> {
        self.kind.rules_mut()
    }

    /// Reset all in-progress within-list drag state. Called on drop; the editor's
    /// drag state also auto-clears when the whole session is torn down
    /// (`playlist_editor = None`).
    pub fn clear_drag(&mut self) {
        self.drag_source = None;
        self.drag_cursor = None;
        self.drag_edge = crate::widgets::drag_column::EdgeZone::None;
        self.drag_target_slot = None;
    }
}
