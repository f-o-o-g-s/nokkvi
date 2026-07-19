//! Rules-session domain state (iced-free) — what the UI's rules editor
//! session is ABOUT, as opposed to how it renders (`src/state/` owns that).

/// What a rules session edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesTarget {
    /// Creating a new smart playlist. Carries no name/public payload — the
    /// one-screen create flow seeds those in the session UI state
    /// (placeholder name, `public: false`) and the edit-bar collects them;
    /// the finalize is where the name lands.
    Create,
    /// Editing an existing smart playlist's rules.
    Edit {
        playlist_id: String,
        /// A scanner-synced file backs this playlist — its file's rules
        /// overwrite API rule edits on every scan while `sync` holds.
        file_backed: bool,
        sync: bool,
        /// The optimistic-concurrency token captured at session open (the
        /// Tracks editor's `loaded_updated_at` pattern): Save re-checks the
        /// server's current `updatedAt` and surfaces the conflict flow on
        /// mismatch instead of silently clobbering a concurrent edit.
        loaded_updated_at: String,
    },
}

/// The private draft workspace playlist backing server-truth previews.
/// `None` until the first draft POST — which, for blank creates, happens at
/// the first validation-passing Preview press, never at session open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftInfo {
    /// The draft playlist's server id.
    pub id: String,
    /// The strict-grammar marker comment the draft carries
    /// (`nokkvi-draft/<version> pid=<pid> ts=<ts>` — see
    /// [`crate::types::playlist::DraftMarker`]). Every draft write mints a
    /// fresh `ts`, so an actively-previewing session never ages out of the
    /// orphan sweep's protection.
    pub marker: String,
}

/// The display name every draft is created under — drafts ARE visible in
/// other clients (web UI, Feishin, phone) during a session; the name
/// self-explains a sighting.
pub const DRAFT_DISPLAY_NAME: &str = "nokkvi draft (safe to ignore)";
